/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use crate::auth;
use crate::error::{AppError, AppResult};
use crate::models::{
    CreateUserRequest, LoginResponse, RechargeRequest, UpdateUserRequest, User, UserListResponse,
};
use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> AppResult<Json<UserListResponse>> {
    let keyword = query
        .get("keyword")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let users: Vec<User> = if let Some(kw) = keyword {
        let like_pattern = format!("%{}%", kw);
        let limit: i64 = query
            .get("limit")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50)
            .clamp(1, 100);
        sqlx::query_as(&state.db.format_query(
            "SELECT u.*, ul.name as level_name, ul.id as level_id FROM users u \
             LEFT JOIN user_levels ul ON u.user_group = ul.group_key \
             WHERE u.role = 'user' AND (u.username LIKE ? OR u.uid LIKE ? OR u.email LIKE ? OR u.nickname LIKE ?) \
             ORDER BY u.created_at DESC LIMIT ?"
        ))
        .bind(&like_pattern)
        .bind(&like_pattern)
        .bind(&like_pattern)
        .bind(&like_pattern)
        .bind(limit)
        .fetch_all(&state.db.pool)
        .await?
    } else {
        sqlx::query_as(&state.db.format_query(
            "SELECT u.*, ul.name as level_name FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key ORDER BY u.created_at DESC"
        ))
        .fetch_all(&state.db.pool)
        .await?
    };

    let total = users.len() as i64;
    Ok(Json(UserListResponse { data: users, total }))
}

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(claims): axum::extract::Extension<crate::auth::Claims>,
    Json(request): Json<CreateUserRequest>,
) -> AppResult<Json<User>> {
    let operator_name = claims.username.clone();
    crate::api::auth::validate_username(&request.username, false)?;
    let mut actual_email = request.email.clone();
    if actual_email.is_empty() {
        use rand::Rng;
        let random_suffix: String = (0..8)
            .map(|_| rand::thread_rng().gen_range(0..10).to_string())
            .collect();
        actual_email = format!("u_{}@tokensbyte.local", random_suffix);
    }

    let exists: bool = sqlx::query_scalar(
        &state
            .db
            .format_query("SELECT EXISTS(SELECT 1 FROM users WHERE username = ? OR email = ?)"),
    )
    .bind(&request.username)
    .bind(&actual_email)
    .fetch_one(&state.db.pool)
    .await?;

    if exists {
        return Err(AppError::Conflict("User already exists".to_string()));
    }

    let password_hash = auth::hash_password(&request.password)?;
    let user_id = uuid::Uuid::new_v4().to_string();
    let uid = state
        .db
        .generate_unique_uid()
        .await
        .map_err(AppError::from)?;

    let role = request.role.as_deref().unwrap_or("user");
    let user_group = request
        .user_group
        .as_deref()
        .unwrap_or(request.group.as_deref().unwrap_or("default"));
    let admin_group_id = request.admin_group_id;
    let mut referred_by = request.referred_by.clone().or(request.aff.clone());

    // Resolve referred_by to ID if it's a UID or Username
    if let Some(ref ref_val) = referred_by {
        if !ref_val.trim().is_empty() {
            let resolved_id: Option<String> = sqlx::query_scalar(&state.db.format_query(
                "SELECT id FROM users WHERE id = ? OR uid = ? OR username = ? LIMIT 1",
            ))
            .bind(ref_val)
            .bind(ref_val)
            .bind(ref_val)
            .fetch_optional(&state.db.pool)
            .await?;

            if let Some(id) = resolved_id {
                referred_by = Some(id);
            }
        }
    }

    let referral_history = if let Some(ref inviter_id) = referred_by {
        let now = crate::time_system::utc_naive_string();
        let display_name = state.db.get_user_display_name(inviter_id).await;
        Some(format!("[{}] 通过 {} 邀请注册\n", now, display_name))
    } else {
        None
    };

    let balance = request.balance.unwrap_or(0.0);
    let gift_balance = request.gift_balance.unwrap_or(0.0);
    let pay_enabled = request.pay_enabled.unwrap_or(1);
    // 管理员建号时昵称为可选，缺省仍为 NULL（列表回退到 username）
    let nickname = request
        .nickname
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let mut tx = state.db.pool.begin().await?;

    sqlx::query(
        &state.db.format_query(r#"INSERT INTO users (id, uid, username, email, mobile, password_hash, role, user_group, admin_group_id, balance, gift_balance, pay_enabled, is_active, referred_by, referral_history, nickname)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)"#)
    )
    .bind(&user_id)
    .bind(&uid)
    .bind(&request.username)
    .bind(&actual_email)
    .bind(&request.mobile)
    .bind(&password_hash)
    .bind(role)
    .bind(user_group)
    .bind(admin_group_id)
    .bind(balance)
    .bind(gift_balance)
    .bind(pay_enabled)
    .bind(&referred_by)
    .bind(&referral_history)
    .bind(&nickname)
    .execute(&mut *tx)
    .await?;

    if balance > 0.0 {
        sqlx::query(
            &state.db.format_query("INSERT INTO recharge_records (user_id, amount, recharge_type, remark, operator, wallet_type) VALUES (?, ?, 'manual', ?, ?, 'system')")
        )
        .bind(&user_id)
        .bind(balance)
        .bind("管理员创建用户-系统余额")
        .bind(&operator_name)
        .execute(&mut *tx)
        .await?;
    }

    if gift_balance > 0.0 {
        sqlx::query(
            &state.db.format_query("INSERT INTO recharge_records (user_id, amount, recharge_type, remark, operator, wallet_type) VALUES (?, ?, 'manual', ?, ?, 'gift')")
        )
        .bind(&user_id)
        .bind(gift_balance)
        .bind("管理员创建用户-赠送余额")
        .bind(&operator_name)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let user: User = sqlx::query_as(&state.db.format_query(
        "SELECT u.*, ul.name as level_name FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key WHERE u.id = ?"
    ))
    .bind(&user_id)
    .fetch_one(&state.db.pool)
    .await?;

    Ok(Json(user))
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Extension(claims): axum::extract::Extension<crate::auth::Claims>,
    Json(request): Json<UpdateUserRequest>,
) -> AppResult<Json<User>> {
    let operator_name = claims.username.clone();
    let operator_id = claims.sub.clone();
    let mut user: User = sqlx::query_as(&state.db.format_query(
        "SELECT u.*, ul.name as level_name FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key WHERE u.id = ?"
    ))
    .bind(&id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    let old_balance = user.balance;
    let old_gift_balance = user.gift_balance;
    let old_credit_limit = user.credit_limit;

    // 管理员账号变更保护（避免绕过界面直接调接口改其他管理员）：
    // 1) 目标是管理员时，操作者必须是超级管理员（库内 role=admin 且未绑定管理员分组）；
    // 2) 目标是超级管理员时，禁止修改其角色与分组绑定，避免被降级或解绑架空。
    if user.role == "admin" {
        let operator: Option<(String, Option<i64>)> =
            sqlx::query_as::<_, (String, Option<i64>)>(
                &state
                    .db
                    .format_query("SELECT role, admin_group_id FROM users WHERE id = ?"),
            )
            .bind(&operator_id)
            .fetch_optional(&state.db.pool)
            .await?;
        let is_super_operator = operator
            .as_ref()
            .map(|(role, group)| role == "admin" && group.is_none())
            .unwrap_or(false);
        if !is_super_operator {
            return Err(AppError::Forbidden(
                "仅超级管理员可修改管理员账号".to_string(),
            ));
        }
        if user.admin_group_id.is_none() {
            let changes_role = request
                .role
                .as_deref()
                .map(|role| role != "admin")
                .unwrap_or(false);
            let changes_group = matches!(request.admin_group_id, Some(Some(_)));
            if changes_role || changes_group {
                return Err(AppError::Forbidden(
                    "超级管理员的角色与管理员分组不允许修改".to_string(),
                ));
            }
        }
    }
    // 记录旧等级信息，用于变更时写日志
    let old_user_group = user.user_group.clone();
    let old_level_name: String = sqlx::query_scalar(
        &state
            .db
            .format_query("SELECT COALESCE(name, ?) FROM user_levels WHERE group_key = ?"),
    )
    .bind(&old_user_group)
    .bind(&old_user_group)
    .fetch_optional(&state.db.pool)
    .await?
    .flatten()
    .unwrap_or_else(|| old_user_group.clone());

    if let Some(username) = request.username.filter(|u| u != &user.username) {
        crate::api::auth::validate_username(&username, false)?;
        user.username = username;
    }
    if let Some(email) = request.email {
        // 管理员改邮箱也需保证唯一，避免重复邮箱导致找回密码一次改多个账号
        if !email.is_empty() && !email.ends_with("@tokensbyte.local") {
            let exists: bool =
                sqlx::query_scalar(&state.db.format_query(
                    "SELECT EXISTS(SELECT 1 FROM users WHERE email = ? AND id != ?)",
                ))
                .bind(&email)
                .bind(&user.id)
                .fetch_one(&state.db.pool)
                .await?;
            if exists {
                return Err(AppError::Conflict("该邮箱已被其他账号使用".to_string()));
            }
        }
        user.email = email;
    }
    if let Some(password) = request.password {
        user.password_hash = auth::hash_password(&password)?;
    }
    if let Some(ref nickname) = request.nickname {
        if nickname.chars().count() > 24 {
            return Err(AppError::BadRequest(
                "昵称长度最多不能超过 24 个字符".to_string(),
            ));
        }
        user.nickname = Some(nickname.clone());
    }
    if let Some(mobile) = request.mobile {
        if !mobile.is_empty() {
            let exists: bool =
                sqlx::query_scalar(&state.db.format_query(
                    "SELECT EXISTS(SELECT 1 FROM users WHERE mobile = ? AND id != ?)",
                ))
                .bind(&mobile)
                .bind(&user.id)
                .fetch_one(&state.db.pool)
                .await?;
            if exists {
                return Err(AppError::Conflict("该手机号已被其他账号绑定".to_string()));
            }
        }
        user.mobile = Some(mobile);
    }
    if let Some(wechat_id) = request.wechat_id {
        user.wechat_id = Some(wechat_id);
    }
    if let Some(role) = request.role {
        user.role = role;
    }
    if let Some(balance) = request.balance {
        user.balance = balance;
    }
    if let Some(gift_balance) = request.gift_balance {
        user.gift_balance = gift_balance;
    }
    if let Some(gift_used_quota) = request.gift_used_quota {
        user.gift_used_quota = gift_used_quota;
    }
    if let Some(user_group) = request.user_group {
        user.user_group = user_group;
    }
    if let Some(is_active) = request.is_active {
        user.is_active = is_active;
    }
    if let Some(admin_remark) = request.admin_remark {
        user.admin_remark = Some(admin_remark);
    }
    if let Some(admin_group_opt) = request.admin_group_id {
        user.admin_group_id = admin_group_opt;
    }
    if user.role != "admin" {
        user.admin_group_id = None;
    }
    // 用户模型单独折扣（空字符串视为清空，存 NULL）
    // 口径 v2：已有 mid 键的**值**不许被即时保存改写，只能走「模型折扣」页的预约生效；
    // 新增键（首次配置）与删键不受限。服务端必须拦，否则直接调接口即可绕过预约。
    if let Some(ref md) = request.model_discounts {
        // 空字符串 = 清空（存 NULL），与既有语义一致；先落成具名值再解析，避开临时值借用
        let incoming_raw: Option<String> = if md.is_empty() { None } else { Some(md.clone()) };
        let incoming = crate::api::user_discount_schedules::parse_model_discounts(&incoming_raw);
        let existing = crate::api::user_discount_schedules::parse_model_discounts(&user.model_discounts);
        let changed: Vec<String> = existing
            .keys()
            .filter(|mid| {
                match (existing.get(*mid), incoming.get(*mid)) {
                    (Some(old_v), Some(new_v)) => (old_v - new_v).abs() > 1e-9,
                    _ => false,
                }
            })
            .cloned()
            .collect();
        if !changed.is_empty() {
            return Err(AppError::BadRequest(format!(
                "以下 MID 已有折扣，修改请走「模型折扣」页的预约生效（{}）",
                changed.join("、")
            )));
        }
        user.model_discounts = incoming_raw;
    }
    // 用户级 IP 白名单（逗号分隔 IP/CIDR；空字符串=不限）
    if let Some(ref ips) = request.allowed_ips {
        let trimmed = ips.trim();
        user.allowed_ips = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
    if let Some(timezone) = request.timezone {
        user.timezone = Some(timezone);
    }
    if let Some(credit_limit) = request.credit_limit {
        user.credit_limit = credit_limit;
    }
    if let Some(pay_enabled) = request.pay_enabled {
        user.pay_enabled = pay_enabled;
    }
    let old_referred_by = user.referred_by.clone();

    if let Some(referred_by) = request.referred_by {
        let mut new_ref = if referred_by.trim().is_empty() {
            None
        } else {
            Some(referred_by.clone())
        };

        // Resolve referred_by to ID if it's a UID or Username
        if let Some(ref ref_val) = new_ref {
            let resolved_id: Option<String> = sqlx::query_scalar(&state.db.format_query(
                "SELECT id FROM users WHERE id = ? OR uid = ? OR username = ? LIMIT 1",
            ))
            .bind(ref_val)
            .bind(ref_val)
            .bind(ref_val)
            .fetch_optional(&state.db.pool)
            .await?;

            if let Some(id) = resolved_id {
                new_ref = Some(id);
            }
        }

        if old_referred_by != new_ref {
            let now = crate::time_system::utc_naive_string();
            let old_str = state
                .db
                .get_user_display_name(&old_referred_by.unwrap_or_else(|| "无".to_string()))
                .await;
            let new_str = state
                .db
                .get_user_display_name(&new_ref.clone().unwrap_or_else(|| "无".to_string()))
                .await;
            let msg = format!("[{}] 推荐人从 {} 变更为 {}\n", now, old_str, new_str);
            let mut hist = user.referral_history.clone().unwrap_or_default();
            hist.push_str(&msg);
            user.referral_history = Some(hist);
        }
        user.referred_by = new_ref;
    }

    let mut tx = state.db.pool.begin().await?;

    sqlx::query(
        &state.db.format_query(r#"UPDATE users SET username = ?, email = ?, password_hash = ?, 
           nickname = ?, mobile = ?, wechat_id = ?,
           role = ?, admin_group_id = ?, balance = ?, gift_balance = ?, gift_used_quota = ?, user_group = ?, is_active = ?, admin_remark = ?, referred_by = ?, referral_history = ?, model_discounts = ?, allowed_ips = ?, timezone = ?, credit_limit = ?, pay_enabled = ?, updated_at = CURRENT_TIMESTAMP
           WHERE id = ?"#)
    )
    .bind(&user.username)
    .bind(&user.email)
    .bind(&user.password_hash)
    .bind(&user.nickname)
    .bind(&user.mobile)
    .bind(&user.wechat_id)
    .bind(&user.role)
    .bind(user.admin_group_id)
    .bind(user.balance)
    .bind(user.gift_balance)
    .bind(user.gift_used_quota)
    .bind(&user.user_group)
    .bind(user.is_active)
    .bind(&user.admin_remark)
    .bind(&user.referred_by)
    .bind(&user.referral_history)
    .bind(&user.model_discounts)
    .bind(&user.allowed_ips)
    .bind(&user.timezone)
    .bind(user.credit_limit)
    .bind(user.pay_enabled)
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 系统钱包余额变动记录
    if (user.balance - old_balance).abs() > 1e-9 {
        let diff = user.balance - old_balance;
        let remark = if diff > 0.0 {
            "管理员调增系统余额"
        } else {
            "管理员调减系统余额"
        };
        sqlx::query(
            &state.db.format_query("INSERT INTO recharge_records (user_id, amount, recharge_type, remark, operator, wallet_type) VALUES (?, ?, 'manual', ?, ?, 'system')")
        )
        .bind(&id)
        .bind(diff)
        .bind(remark)
        .bind(&operator_name)
        .execute(&mut *tx)
        .await?;
    }

    // 赠送钱包余额变动记录
    if (user.gift_balance - old_gift_balance).abs() > 1e-9 {
        let diff = user.gift_balance - old_gift_balance;
        let remark = if diff > 0.0 {
            "管理员调增赠送余额"
        } else {
            "管理员调减赠送余额"
        };
        sqlx::query(
            &state.db.format_query("INSERT INTO recharge_records (user_id, amount, recharge_type, remark, operator, wallet_type) VALUES (?, ?, 'manual', ?, ?, 'gift')")
        )
        .bind(&id)
        .bind(diff)
        .bind(remark)
        .bind(&operator_name)
        .execute(&mut *tx)
        .await?;
    }

    // 信控额度变动记录
    if (user.credit_limit - old_credit_limit).abs() > 1e-9 {
        let diff = user.credit_limit - old_credit_limit;
        let remark = if diff > 0.0 {
            "管理员调增信控额度"
        } else {
            "管理员调减信控额度"
        };
        sqlx::query(
            &state.db.format_query("INSERT INTO recharge_records (user_id, amount, recharge_type, remark, operator, wallet_type) VALUES (?, ?, 'manual', ?, ?, 'credit')")
        )
        .bind(&id)
        .bind(diff)
        .bind(remark)
        .bind(&operator_name)
        .execute(&mut *tx)
        .await?;
    }

    // 用户等级变更日志
    if user.user_group != old_user_group {
        let new_level_name: String = sqlx::query_scalar(
            &state
                .db
                .format_query("SELECT COALESCE(name, ?) FROM user_levels WHERE group_key = ?"),
        )
        .bind(&user.user_group)
        .bind(&user.user_group)
        .fetch_optional(&mut *tx)
        .await?
        .flatten()
        .unwrap_or_else(|| user.user_group.clone());

        sqlx::query(&state.db.format_query(
            "INSERT INTO user_level_logs (user_id, old_level, old_level_name, new_level, new_level_name, operator, operator_id, source) VALUES (?, ?, ?, ?, ?, ?, ?, 'admin')"
        ))
        .bind(&id)
        .bind(&old_user_group)
        .bind(&old_level_name)
        .bind(&user.user_group)
        .bind(&new_level_name)
        .bind(&operator_name)
        .bind(&operator_id)
        .execute(&mut *tx)
        .await?;
    }

    let balance_changed = (user.balance - old_balance).abs() > 1e-9
        || (user.gift_balance - old_gift_balance).abs() > 1e-9;

    tx.commit().await?;

    // 用户级 IP 白名单改动立即生效，不等缓存 TTL 过期
    state.user_limits.invalidate(&id);

    if balance_changed {
        crate::services::notification::spawn_low_balance_check(Arc::clone(&state), id.clone());
    }

    Ok(Json(user))
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Extension(claims): axum::extract::Extension<crate::auth::Claims>,
) -> AppResult<Json<serde_json::Value>> {
    // 防止管理员删除自己
    if claims.sub == id {
        return Err(AppError::BadRequest(
            "不能删除当前登录的管理员账户".to_string(),
        ));
    }

    // 管理员账号删除保护：
    // 1) 超级管理员（role=admin 且未绑定管理员分组）任何情况下都不可删除；
    // 2) 删除其他管理员账号时，操作者也必须是超级管理员，
    //    避免分组管理员互删，也避免绕过前端界面直接调接口越权删除。
    let target: Option<(String, Option<i64>)> =
        sqlx::query_as::<_, (String, Option<i64>)>(
            &state
                .db
                .format_query("SELECT role, admin_group_id FROM users WHERE id = ?"),
        )
        .bind(&id)
        .fetch_optional(&state.db.pool)
        .await?;
    let (target_role, target_group_id) =
        target.ok_or_else(|| AppError::NotFound("用户不存在".to_string()))?;

    if target_role == "admin" {
        // 以库内实时数据为准，不信任 JWT 中的 role（防止降级后旧 token 仍可用）
        let operator: Option<(String, Option<i64>)> =
            sqlx::query_as::<_, (String, Option<i64>)>(
                &state
                    .db
                    .format_query("SELECT role, admin_group_id FROM users WHERE id = ?"),
            )
            .bind(&claims.sub)
            .fetch_optional(&state.db.pool)
            .await?;
        let is_super_operator = operator
            .as_ref()
            .map(|(role, group)| role == "admin" && group.is_none())
            .unwrap_or(false);
        if !is_super_operator {
            return Err(AppError::Forbidden(
                "仅超级管理员可删除管理员账号".to_string(),
            ));
        }
        if target_group_id.is_none() {
            return Err(AppError::Forbidden(
                "超级管理员账号不允许删除".to_string(),
            ));
        }
    }

    // 0. 在事务开启前，先查出该用户在 plugin_assets 与 playground_assets 中上传的全部云端文件，
    //    以进行同步且并发的物理清理，避免云端对象存储产生孤儿垃圾文件。
    //    注：不可在下面的数据库事务内执行网络 IO，以避免长时间占用数据库连接。
    let assets: Vec<(String, String)> = sqlx::query_as::<_, (String, String)>(
        &state
            .db
            .format_query("SELECT file_url, plugin_ns FROM plugin_assets WHERE user_id = ?"),
    )
    .bind(&id)
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    let pg_assets: Vec<String> = sqlx::query_scalar::<_, String>(
        &state.db.format_query("SELECT tos_object_key FROM playground_assets WHERE user_id = ? AND tos_object_key IS NOT NULL AND tos_object_key != ''")
    )
    .bind(&id)
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    let pg2026_assets: Vec<String> = sqlx::query_scalar::<_, String>(
        &state.db.format_query("SELECT tos_object_key FROM playground_2026_project_assets WHERE user_id = ? AND tos_object_key IS NOT NULL AND tos_object_key != ''")
    )
    .bind(&id)
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    if !assets.is_empty() || !pg_assets.is_empty() || !pg2026_assets.is_empty() {
        // 并发生成 TOS 文件删除任务
        let mut delete_tasks = Vec::new();

        // 1. 处理 plugin_assets 关联的 TOS 文件删除 (包含国内版与国际版素材管理等插件)
        if !assets.is_empty() {
            // 缓存不同 plugin_ns 的 TosConfig，避免重复查库
            let mut tos_configs = std::collections::HashMap::new();
            for ns in assets
                .iter()
                .map(|(_, ns)| ns)
                .collect::<std::collections::HashSet<_>>()
            {
                if let Some(config) = crate::api::plugins::get_tos_config(&state, ns).await {
                    tos_configs.insert(ns.clone(), config);
                }
            }

            for (file_url, plugin_ns) in assets {
                if let Some(tos_config) = tos_configs.get(&plugin_ns).cloned() {
                    if let Some(object_key) = tos_config.extract_object_key(&file_url) {
                        let task = tokio::spawn(async move {
                            match crate::services::tos::delete_file(&tos_config, &object_key).await
                            {
                                Ok(()) => {
                                    tracing::info!(
                                        "同步清理用户数据: plugin_assets TOS 文件删除成功: {}",
                                        object_key
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "同步清理用户数据: plugin_assets TOS 文件删除失败: {} - {}",
                                        object_key,
                                        e
                                    );
                                }
                            }
                        });
                        delete_tasks.push(task);
                    }
                }
            }
        }

        // 2. 处理 playground_assets (体验中心) 关联的 TOS 文件删除
        if !pg_assets.is_empty() {
            if let Some(tos_config) =
                crate::api::plugins::get_tos_config(&state, "playground").await
            {
                for tos_key in pg_assets {
                    let tos_config = tos_config.clone();
                    let task = tokio::spawn(async move {
                        match crate::services::tos::delete_file(&tos_config, &tos_key).await {
                            Ok(()) => {
                                tracing::info!(
                                    "同步清理用户数据: playground_assets TOS 文件删除成功: {}",
                                    tos_key
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "同步清理用户数据: playground_assets TOS 文件删除失败: {} - {}",
                                    tos_key,
                                    e
                                );
                            }
                        }
                    });
                    delete_tasks.push(task);
                }
            }
        }

        // 2b. 处理 playground_2026_project_assets (创作中心2026) 关联的 TOS 文件删除
        if !pg2026_assets.is_empty() {
            if let Some(tos_config) =
                crate::api::plugins::get_tos_config(&state, "playground_2026").await
            {
                for tos_key in pg2026_assets {
                    let tos_config = tos_config.clone();
                    let task = tokio::spawn(async move {
                        match crate::services::tos::delete_file(&tos_config, &tos_key).await {
                            Ok(()) => {
                                tracing::info!(
                                    "同步清理用户数据: playground_2026_project_assets TOS 文件删除成功: {}",
                                    tos_key
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "同步清理用户数据: playground_2026_project_assets TOS 文件删除失败: {} - {}",
                                    tos_key,
                                    e
                                );
                            }
                        }
                    });
                    delete_tasks.push(task);
                }
            }
        }

        // 超时保障机制：使用 tokio::time::timeout 给并发执行 of tasks 设置一个 30 秒的最大时间上限。
        // 即使云端网络异常卡死，接口也将在 30 秒内强制返回并继续主流程，绝不阻塞用户删除接口的正常响应。
        if !delete_tasks.is_empty() {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                futures::future::join_all(delete_tasks),
            )
            .await;
        }
    }

    // 使用事务，按外键依赖顺序逐层清理关联数据
    let mut tx = state.db.pool.begin().await?;

    // 1. commissions 引用 recharge_records(id) 和 users(id)，必须最先删
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM commissions WHERE user_id = ? OR from_user_id = ?"),
    )
    .bind(&id)
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 2. recharge_records 引用 users(id)
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM recharge_records WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 3. api_tokens 引用 users(id)
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM api_tokens WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 4. orders 引用 users(id)
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM orders WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 5. plugin_assets 引用 users(id)
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM plugin_assets WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 6. plugin_asset_groups 引用 users(id)
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM plugin_asset_groups WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 7. 无外键但需清理的业务数据
    sqlx::query(&state.db.format_query("DELETE FROM logs WHERE user_id = ?"))
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM plugin_api_logs WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    // marketing 关联（无外键但需清理）
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM marketing_team_leaders WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        &state
            .db
            .format_query("DELETE FROM marketing_team_members WHERE user_id = ?"),
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;

    // 8. 最终删除用户主记录（playground_projects / playground_assets 已有 ON DELETE CASCADE）
    sqlx::query(&state.db.format_query("DELETE FROM users WHERE id = ?"))
        .bind(&id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(Json(serde_json::json!({ "success": true })))
}
pub async fn recharge_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Extension(claims): axum::extract::Extension<crate::auth::Claims>,
    Json(request): Json<RechargeRequest>,
) -> AppResult<Json<User>> {
    if request.amount.is_nan() || request.amount.is_infinite() {
        return Err(AppError::BadRequest("无效的金额数值".to_string()));
    }
    let request_amount = crate::money::round_money(request.amount);

    let operator_name = claims.username.clone();
    let mut tx = state.db.pool.begin().await?;

    let user: User = sqlx::query_as(&state.db.format_query(
        "SELECT u.*, ul.name as level_name FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key WHERE u.id = ?"
    ))
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    let remark = request
        .remark
        .unwrap_or_else(|| "Administrator Adjustment".to_string());
    let is_gift = request.wallet_type == "gift";
    let is_credit = request.wallet_type == "credit";

    if is_credit {
        // 信控额度操作
        let new_credit = user.credit_limit + request_amount;
        if new_credit < 0.0 {
            return Err(AppError::BadRequest("信控额度不能为负数".to_string()));
        }
        sqlx::query(&state.db.format_query(
            "UPDATE users SET credit_limit = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        ))
        .bind(new_credit)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    } else if is_gift {
        // 赠送钱包操作
        let new_gift = user.gift_balance + request_amount;
        sqlx::query(&state.db.format_query(
            "UPDATE users SET gift_balance = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        ))
        .bind(new_gift)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    } else {
        // 系统钱包操作
        let new_balance = user.balance + request_amount;
        sqlx::query(&state.db.format_query(
            "UPDATE users SET balance = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        ))
        .bind(new_balance)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    }

    let recharge_type = "manual";
    let wallet_type = if is_credit {
        "credit"
    } else if is_gift {
        "gift"
    } else {
        "system"
    };
    let recharge_id: i64 = sqlx::query_scalar::<_, i64>(&state.db.format_query("INSERT INTO recharge_records (user_id, amount, recharge_type, remark, operator, wallet_type) VALUES (?, ?, ?, ?, ?, ?) RETURNING id"))
        .bind(&id)
        .bind(request_amount)
        .bind(recharge_type)
        .bind(&remark)
        .bind(&operator_name)
        .bind(wallet_type)
        .fetch_one(&mut *tx)
        .await?;

    // 系统钱包且为正金额时才奖励佣金，赠送钱包和扣减操作不计入佣金范围
    if !is_gift && request_amount > 0.0 {
        if let Err(e) = crate::services::affiliate::award_commission(
            &state.db,
            &mut tx,
            &id,
            recharge_id,
            request_amount,
        )
        .await
        {
            tracing::error!(
                "Failed to award commission for recharge {}: {}",
                recharge_id,
                e
            );
        }
    }

    tx.commit().await?;

    // 扣减可能触发提醒；充值回升需清除本轮标记
    crate::services::notification::spawn_low_balance_check(Arc::clone(&state), id.clone());

    let updated_user: User = sqlx::query_as(&state.db.format_query(
        "SELECT u.*, ul.name as level_name FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key WHERE u.id = ?"
    ))
    .bind(&id)
    .fetch_one(&state.db.pool)
    .await?;

    Ok(Json(updated_user))
}

pub async fn impersonate_user(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(claims): axum::extract::Extension<crate::auth::Claims>,
    Path(id): Path<String>,
) -> AppResult<Json<LoginResponse>> {
    // 防御纵深：路由层已有 auth + admin_middleware，handler 再强制校验管理员身份
    if claims.role != "admin" {
        return Err(AppError::Forbidden("Admin access required".to_string()));
    }

    let user: User = sqlx::query_as(&state.db.format_query(
        "SELECT u.*, ul.name as level_name FROM users u LEFT JOIN user_levels ul ON u.user_group = ul.group_key WHERE u.id = ?"
    ))
    .bind(&id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    if user.role != "user" {
        return Err(AppError::Forbidden(
            "Only normal users can be impersonated".to_string(),
        ));
    }

    if user.is_active == 0 {
        return Err(AppError::Forbidden("Account disabled".to_string()));
    }

    tracing::warn!(
        "[Impersonate] admin={} ({}) impersonating user={} ({})",
        claims.username,
        claims.sub,
        user.username,
        user.id
    );

    let token = auth::create_token(
        &user.id,
        &user.username,
        &user.role,
        &state.config.jwt_secret,
    )?;

    Ok(Json(LoginResponse { token, user }))
}

/// 管理员查询指定用户的等级变更历史
pub async fn get_user_level_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let logs = sqlx::query(
        &state.db.format_query(
            "SELECT id, user_id, old_level, old_level_name, new_level, new_level_name, operator, operator_id, source, remark, created_at FROM user_level_logs WHERE user_id = ? ORDER BY created_at DESC LIMIT 100"
        )
    )
    .bind(&id)
    .fetch_all(&state.db.pool)
    .await?;

    use sqlx::Row;
    let data: Vec<serde_json::Value> = logs.iter().map(|row| {
        serde_json::json!({
            "id": row.get::<i64, _>("id"),
            "old_level": row.get::<String, _>("old_level"),
            "old_level_name": row.get::<String, _>("old_level_name"),
            "new_level": row.get::<String, _>("new_level"),
            "new_level_name": row.get::<String, _>("new_level_name"),
            "operator": row.get::<String, _>("operator"),
            "source": row.get::<String, _>("source"),
            "remark": row.get::<String, _>("remark"),
            "created_at": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
        })
    }).collect();

    Ok(Json(
        serde_json::json!({ "data": data, "total": data.len() }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ConsumptionStatsBatchRequest {
    pub user_ids: Vec<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

#[derive(Debug, Serialize, Default, Clone)]
pub struct ConsumptionStatsBatchItem {
    pub system_cost: f64,
    pub gift_cost: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct ConsumptionStatsBatchRawRow {
    user_id: String,
    system_cost: f64,
    gift_cost: f64,
}

/// 批量查询用户消费合计（系统钱包 / 赠送钱包）。
/// 历史天走 usage_daily_stats，今日及碎片段走 logs，与财务/明细口径一致。
pub async fn query_consumption_stats_batch(
    state: &AppState,
    user_ids: &[String],
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> AppResult<HashMap<String, ConsumptionStatsBatchItem>> {
    if user_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let site_tz = crate::relay::relay_settings::get_cached_site_timezone(&state.db).await;
    let tz: chrono_tz::Tz = site_tz.parse().unwrap_or(chrono_tz::Asia::Shanghai);
    let slices =
        crate::api::date_helper::calculate_query_slices(start_date, end_date, tz);

    let placeholders = user_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let mut result: HashMap<String, ConsumptionStatsBatchItem> = HashMap::new();
    let merge_row =
        |map: &mut HashMap<String, ConsumptionStatsBatchItem>, row: ConsumptionStatsBatchRawRow| {
            let entry = map.entry(row.user_id).or_default();
            entry.system_cost += row.system_cost;
            entry.gift_cost += row.gift_cost;
        };

    if slices.has_history_days {
        let sql = format!(
            "SELECT user_id, \
                COALESCE(SUM(GREATEST(total_cost - total_pre_deduct_gift, 0.0)), 0.0) as system_cost, \
                COALESCE(SUM(total_pre_deduct_gift), 0.0) as gift_cost \
             FROM usage_daily_stats \
             WHERE user_id IN ({}) AND {} \
             GROUP BY user_id",
            placeholders,
            slices.history_cond("stat_date")
        );
        let formatted_sql = state.db.format_query(&sql);
        let mut query = sqlx::query_as::<_, ConsumptionStatsBatchRawRow>(&formatted_sql);
        for id in user_ids {
            query = query.bind(id);
        }
        query = query
            .bind(slices.hist_start_date)
            .bind(slices.hist_end_date);
        let rows = query.fetch_all(&state.db.pool).await?;
        for row in rows {
            merge_row(&mut result, row);
        }
    }

    for r_slice in slices.realtime_slices() {
        let sql = format!(
            "SELECT user_id, \
                COALESCE(SUM(GREATEST(cost - pre_deduct_gift, 0.0)), 0.0) as system_cost, \
                COALESCE(SUM(pre_deduct_gift), 0.0) as gift_cost \
             FROM logs \
             WHERE user_id IN ({}) AND {} \
             GROUP BY user_id",
            placeholders,
            r_slice.sql_cond("created_at")
        );
        let formatted_sql = state.db.format_query(&sql);
        let mut query = sqlx::query_as::<_, ConsumptionStatsBatchRawRow>(&formatted_sql);
        for id in user_ids {
            query = query.bind(id);
        }
        query = query.bind(&r_slice.start).bind(&r_slice.end);
        let rows = query.fetch_all(&state.db.pool).await?;
        for row in rows {
            merge_row(&mut result, row);
        }
    }

    Ok(result)
}

/// 管理后台：批量查询用户消费合计。
pub async fn get_consumption_stats_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ConsumptionStatsBatchRequest>,
) -> AppResult<Json<HashMap<String, ConsumptionStatsBatchItem>>> {
    let result = query_consumption_stats_batch(
        &state,
        &req.user_ids,
        req.start_date.as_deref(),
        req.end_date.as_deref(),
    )
    .await?;
    Ok(Json(result))
}
