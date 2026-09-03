/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

/*
 🛡️ 数据库迁移与结构变更开发规范 (人类 & AI 协作指南)
为了确保热重启性能（logs表百万级以上数据时），所有的 DML (回填数据)
或高代价的 DDL 操作必须且只能通过一次性保护机制执行。

【新增表结构变更或一次性数据更新的规则】：
1. 严禁直接在已有的 once_migration! 块中直接修改或追加 SQL（因为老用户已运行过此 ID 将被跳过）。
2. 严禁直接修改存量的 CREATE TABLE 或 DDL 块。
3. 必须在 `pg_migration_blocks!` 宏的【最尾部】新增一个独立的
   `once_migration!(pool, "unique_migration_id_vX", "SQL...");` 块。

【线上兼容 / 避免业务中断】：
1. 类型变更必须「已是目标类型则跳过」（见 timestamptz_unify_v1），禁止假定列为 TEXT。
2. 大表建索引优先 `CREATE INDEX CONCURRENTLY IF NOT EXISTS`（不堵写入）。
3. 失败不得写入 sys_migration_history（once_migration! 已保证），以便下次启动重试。
4. 禁止在迁移里 DROP 仍可能被查询使用的 covering 索引；精简索引应运维确认 idx_scan 后再做。
5. 并发建索引失败可能留下 INVALID 索引：重试前先 DROP INVALID，再重建。
6. PG quirk：`CREATE INDEX CONCURRENTLY IF NOT EXISTS` 在同名索引已存在时仍可能报
   23505 / pg_class_relname_nsp_index；once_migration! 将其视为已成功（幂等）。
7. 半截/损坏索引上 `DROP INDEX CONCURRENTLY` 可能报 XX000（pg_attribute catalog gap）；
   冗余清理用非并发 DROP + EXCEPTION，或由 once_migration! 按成功跳过，避免卡死启动。
8. 模型分类排序保护：`model_providers` / `model_api_providers` / `model_types` 的 `sort_order`
   由管理端配置，升级不得覆盖。种子仅允许 INSERT（WHERE NOT EXISTS / ON CONFLICT 只改
   is_system 等元数据）；回填 logo/remark/name_en 时禁止顺带 SET sort_order。
*/

// 开源版不再 noop 商业相关 SQL：库表/数据允许存在，插件中心由 is_plugin_compiled 过滤展示。
// 此前用 contains("plugin_config"|"playground"|...) 会误伤 HA / 创作中心 / 门户配置迁移。

/// 索引 DDL 幂等：CREATE 名冲突(23505) / DROP 目录缺口(XX000) 可跳过；不吞业务唯一约束错误。
fn is_idempotent_index_ddl_err(err: &sqlx::Error, stmt: &str) -> bool {
    let sqlx::Error::Database(db) = err else {
        return false;
    };
    // code() 返回临时 Cow，须先绑定再 as_deref，否则临时值提前释放
    let code = db.code();
    let code = code.as_deref();
    let msg = db.message();
    let upper = stmt.to_ascii_uppercase();
    if upper.contains("CREATE INDEX")
        && code == Some("23505")
        && db
            .constraint()
            .map(|c| c == "pg_class_relname_nsp_index")
            .unwrap_or_else(|| msg.contains("pg_class_relname_nsp_index"))
    {
        return true;
    }
    upper.contains("DROP INDEX")
        && code == Some("XX000")
        && msg.contains("pg_attribute catalog is missing")
}

/// 一次性迁移执行宏：通过 sys_migration_history 确保仅首次部署执行，后续重启自动跳过。
/// 任一句失败则**不**写入 history，下次启动可重试（避免半成功被永久跳过）。
macro_rules! once_migration {
    ($pool:expr, $id:literal, $( $stmt:expr ),+ $(,)?) => {{
        let _m_done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sys_migration_history WHERE id = $1")
            .bind($id)
            .fetch_one($pool)
            .await
            .unwrap_or(0);
        if _m_done == 0 {
            let mut _m_ok = true;
            $(
                if _m_ok {
                    match sqlx::query($stmt).execute($pool).await {
                        Ok(_) => {}
                        Err(e) if is_idempotent_index_ddl_err(&e, $stmt) => {
                            tracing::warn!(
                                "⚠️ [Migration] ID: {} 索引 DDL 已满足/目录异常可跳过. 语句: '{}'",
                                $id,
                                $stmt
                            );
                        }
                        Err(e) => {
                            _m_ok = false;
                            tracing::error!(
                                "❌ [Migration] ID: {} 失败，将不写入 history 以便重试. 语句: '{}' 错误: {:?}",
                                $id,
                                $stmt,
                                e
                            );
                        }
                    }
                }
            )+
            if _m_ok {
                let _ = sqlx::query("INSERT INTO sys_migration_history (id) VALUES ($1)")
                    .bind($id)
                    .execute($pool)
                    .await;
                tracing::info!("✅ [Migration] 一次性迁移完成: {}", $id);
            } else {
                tracing::error!("❌ [Migration] 一次性迁移中止（未标记完成）: {}", $id);
            }
        }
    }};
}

macro_rules! pg_migration_blocks {
    ($pool:expr) => {{
        let pool = $pool;

        // 确保一次性迁移历史记录表存在，以便于安全执行列重命名等一次性变更
        let history_table_exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'sys_migration_history'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        if history_table_exists == 0 {
            sqlx::query("CREATE TABLE sys_migration_history (id TEXT PRIMARY KEY, executed_at TEXT NOT NULL DEFAULT (now()::text))").execute(pool).await.ok();
        }

    // Users table
    // ── 初始化核心基础表（受一次性迁移保护） ──
    once_migration!(pool, "init_core_tables_v1",
        r#"CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            uid TEXT NOT NULL UNIQUE,
            username VARCHAR(48) NOT NULL UNIQUE,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            nickname VARCHAR(24),
            mobile TEXT,
            wechat_id TEXT,
            role TEXT NOT NULL DEFAULT 'user' CHECK(role IN ('admin', 'user')),
            balance DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            user_group TEXT NOT NULL DEFAULT 'default',
            used_quota DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            referred_by TEXT, commission_balance DOUBLE PRECISION NOT NULL DEFAULT 0.0, admin_group_id INTEGER,
            register_ip TEXT DEFAULT '',
            admin_remark TEXT DEFAULT '',
            referral_history TEXT DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS recharge_records (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            amount DOUBLE PRECISION NOT NULL,
            recharge_type TEXT NOT NULL DEFAULT 'other',
            remark TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS channels (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            provider_type TEXT NOT NULL,
            base_url TEXT NOT NULL,
            api_key TEXT NOT NULL,
            models TEXT NOT NULL DEFAULT '[]',
            model_mapping TEXT NOT NULL DEFAULT '{}',
            priority INTEGER NOT NULL DEFAULT 0,
            weight INTEGER NOT NULL DEFAULT 1,
            status INTEGER NOT NULL DEFAULT 1,
            balance DOUBLE PRECISION,
            max_rps INTEGER DEFAULT 0,
            quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1,
            quota_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            config TEXT NOT NULL DEFAULT '{}',
            user_groups TEXT NOT NULL DEFAULT '[]',
            group_aid TEXT DEFAULT '',
            preset_id INTEGER,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS api_tokens (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            token_key TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL DEFAULT 'default' CHECK (char_length(name) <= 36 AND name ~ '^([^\W_]| )+$'),
            quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1,
            quota_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            allowed_models TEXT NOT NULL DEFAULT '[]',
            allowed_ips TEXT NOT NULL DEFAULT '',
            ip_whitelist TEXT,
            rps_limit INTEGER DEFAULT 0,
            rpm_limit INTEGER DEFAULT 0,
            expires_at TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            daily_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1.0,
            daily_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            weekly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1.0,
            weekly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            monthly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1.0,
            monthly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            last_reset_day TEXT NOT NULL DEFAULT '',
            last_reset_week TEXT NOT NULL DEFAULT '',
            last_reset_month TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS logs (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL,
            channel_id INTEGER,
            token_id INTEGER,
            model TEXT NOT NULL DEFAULT '',
            prompt_tokens INTEGER NOT NULL DEFAULT 0,
            completion_tokens INTEGER NOT NULL DEFAULT 0,
            cached_tokens INTEGER NOT NULL DEFAULT 0,
            cost DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            latency_ms INTEGER NOT NULL DEFAULT 0,
            status_code INTEGER NOT NULL DEFAULT 200,
            endpoint TEXT NOT NULL DEFAULT '',
            error_message TEXT,
            upstream_url TEXT DEFAULT '',
            request_content TEXT,
            response_content TEXT,
            upstream_req_content TEXT,
            is_stream INTEGER NOT NULL DEFAULT 0,
            billing_detail TEXT DEFAULT '',
            billing_pid TEXT DEFAULT '',
            forward_eid TEXT DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS redemptions (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            code TEXT NOT NULL UNIQUE,
            quota DOUBLE PRECISION NOT NULL,
            is_used INTEGER DEFAULT 0,
            used_at TEXT,
            used_by TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL DEFAULT ''
        )"#,
        r#"CREATE TABLE IF NOT EXISTS orders (
            id SERIAL PRIMARY KEY,
            out_trade_no TEXT NOT NULL UNIQUE,
            user_id TEXT NOT NULL REFERENCES users(id),
            payment_method TEXT NOT NULL,
            amount DOUBLE PRECISION NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            trade_no TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            paid_at TEXT
        )"#,
        "DROP TABLE IF EXISTS task_logs",
        r#"CREATE TABLE IF NOT EXISTS plugin_api_logs (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL,
            plugin_name TEXT NOT NULL,
            api_endpoint TEXT NOT NULL,
            request_payload TEXT,
            response_payload TEXT,
            status_code INTEGER,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS user_levels (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            group_key TEXT NOT NULL UNIQUE,
            discount DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            commission_ratio DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            invite_reward_inviter DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            invite_reward_invitee DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            daily_invite_limit INTEGER NOT NULL DEFAULT 10,
            marketing_enabled INTEGER NOT NULL DEFAULT 0,
            max_token_count INTEGER NOT NULL DEFAULT 10,
            sort_order INTEGER NOT NULL DEFAULT 0,
            description TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS verification_codes (
            id SERIAL PRIMARY KEY,
            email TEXT NOT NULL,
            code TEXT NOT NULL,
            purpose TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#
    );

    // Model Providers table
    // ── 初始化服务商与模型管理表（受一次性迁移保护） ──
    once_migration!(pool, "init_provider_tables_v1",
        r#"CREATE TABLE IF NOT EXISTS model_providers (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            sort_order INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            upstream_type TEXT NOT NULL DEFAULT 'other',
            config TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS model_types (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            sort_order INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            upstream_type TEXT NOT NULL DEFAULT 'other',
            config TEXT,
            default_features TEXT DEFAULT '[]',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS models (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            model_id TEXT NOT NULL,
            original_id TEXT NOT NULL DEFAULT '',
            provider_id INTEGER REFERENCES model_providers(id),
            type_id INTEGER REFERENCES model_types(id),
            group_ratios TEXT NOT NULL DEFAULT '{}',
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            description TEXT,
            feature_attributes TEXT DEFAULT '[]',
            upstream_type TEXT NOT NULL DEFAULT 'other',
            config TEXT,
            enable_log_content INTEGER NOT NULL DEFAULT 0,
            forward_rule_ids TEXT,
            billing_rule_id INTEGER,
            pre_deduction DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#
    );

    // Seed default user level
    once_migration!(pool, "seed_default_user_level_v1",
        r#"INSERT INTO user_levels (name, group_key, discount, description)
           VALUES ('默认用户', 'default', 1.0, '普通用户，无折扣')
           ON CONFLICT (group_key) DO NOTHING"#
    );

    // Admin Groups table
    // ── 初始化管理组、佣金表及核心字段扩展（受一次性迁移保护） ──
    once_migration!(pool, "backfill_channels_user_groups_v1",
        r#"CREATE TABLE IF NOT EXISTS admin_groups (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            permissions TEXT,
            description TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS admin_group_id INTEGER",
        "ALTER TABLE admin_groups ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS commission_ratio DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS invite_reward_inviter DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS invite_reward_invitee DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS daily_invite_limit INTEGER NOT NULL DEFAULT 10",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS marketing_enabled INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS is_default INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS max_token_count INTEGER NOT NULL DEFAULT 10",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        r#"CREATE TABLE IF NOT EXISTS commissions (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            from_user_id TEXT NOT NULL REFERENCES users(id),
            recharge_id INTEGER REFERENCES recharge_records(id),
            amount DOUBLE PRECISION NOT NULL,
            ratio DOUBLE PRECISION NOT NULL,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS remark TEXT",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS description TEXT",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS feature_attributes TEXT DEFAULT '[]'",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS enable_log_content INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS is_stream INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS request_content TEXT",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS response_content TEXT",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS upstream_url TEXT DEFAULT ''",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS upstream_req_content TEXT DEFAULT ''",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS billing_detail TEXT DEFAULT ''",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS billing_pid TEXT DEFAULT ''",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS forward_eid TEXT DEFAULT ''",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS user_groups TEXT NOT NULL DEFAULT '[]'",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS group_aid TEXT DEFAULT ''",
        "CREATE INDEX IF NOT EXISTS idx_logs_created_at ON logs(created_at)",
        "CREATE INDEX IF NOT EXISTS idx_logs_user_id ON logs(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_logs_user_created ON logs(user_id, created_at)"
    );

    // ── 初始化转发规则与计费规则系统结构（受一次性迁移保护） ──
    once_migration!(pool, "init_routing_billing_tables_v1",
        r#"CREATE TABLE IF NOT EXISTS forward_rules (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            rule_type TEXT NOT NULL,
            category TEXT NOT NULL DEFAULT '聊天',
            config_json TEXT NOT NULL DEFAULT '{}',
            description TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            upstream_type TEXT NOT NULL DEFAULT 'other',
            config TEXT,
            eid TEXT DEFAULT '',
            is_system INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE model_types ADD COLUMN IF NOT EXISTS default_features TEXT DEFAULT '[]'",
        "INSERT INTO model_types (name, sort_order, is_active, remark) SELECT '向量', 50, 1, '文本向量（Embedding）模型' WHERE NOT EXISTS (SELECT 1 FROM model_types WHERE name = '向量')",
        "INSERT INTO model_types (name, sort_order, is_active, remark) SELECT '排序', 60, 1, '文本排序（Rerank）模型' WHERE NOT EXISTS (SELECT 1 FROM model_types WHERE name = '排序')",
        "INSERT INTO model_types (name, sort_order, is_active, remark) SELECT '视频增强', 35, 1, '视频画质增强与字幕擦除处理模型' WHERE NOT EXISTS (SELECT 1 FROM model_types WHERE name = '视频增强')",
        "ALTER TABLE forward_rules ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT '聊天'",
        "ALTER TABLE forward_rules ADD COLUMN IF NOT EXISTS is_system INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE forward_rules ADD COLUMN IF NOT EXISTS eid TEXT DEFAULT ''",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS forward_rule_ids TEXT",
        r#"INSERT INTO forward_rules (name, rule_type, description, config_json, category, is_system)
        SELECT t.name, t.rule_type, t.description, t.config_json, t.category, t.is_system
        FROM (VALUES
            ('OpenAI 兼容原生通道 (聊天)', 'openai', '标准的按路径聊天透传规则', '{"path_rewrite":{"old":"/v1/chat/completions","new":"/v1/chat/completions"}}', '聊天', 1),
            ('OpenAI 兼容原生通道 (图片)', 'openai', '供图片生成调用的原生通道', '{"path_rewrite":{"old":"/v1/images/generations","new":"/v1/images/generations"}}', '图片', 1),
            ('OpenAI 兼容原生通道 (视频)', 'openai', '供视频生成调用的原生通道', '{"path_rewrite":{"old":"/v1/video/generations","new":"/v1/video/generations"}}', '视频', 1),
            ('Anthropic 原生转化', 'anthropic', '将 OpenAI 格式请求转换为 Anthropic Messages API 格式，接口 /v1/messages', '{"target_type":"anthropic","path_rewrite":{"old":"/v1/chat/completions","new":"/v1/messages"},"auth_type":"x-api-key"}', '聊天', 1),
            ('Google Gemini 原生生图', 'gemini', '将标准的生图请求适配到 Gemini contents 接口', '{"target_type":"gemini_image","path_rewrite":{"old":"/v1/images/generations","new":"/v1beta/models/${model}:generateContent"},"auth_type":"query_key"}', '图片', 1),
            ('Google Gemini 格式转换 (聊天)', 'gemini', '将标准请求转换并适配到 Gemini contents', '{"target_type":"gemini","path_rewrite":{"old":"/v1/chat/completions","new":"/v1beta/models/${model}:generateContent"},"auth_type":"query_key"}', '聊天', 1),
            ('火山方舟 视频生成', 'volcengine', '将标准的视频生成请求适配到火山方舟 tasks 接口', '{"target_type":"volcengine","path_rewrite":{"old":"/v1/video/generations","new":"/api/v3/contents/generations/tasks"},"auth_type":"bearer"}', '视频', 1),
            ('火山方舟 聊天', 'volcengine', '将标准的聊天请求转发到火山方舟官方 Chat 接口，body 保持 OpenAI 兼容格式', '{"target_type":"volcengine_chat","path_rewrite":{"old":"/v1/chat/completions","new":"/api/v3/chat/completions"},"auth_type":"bearer"}', '聊天', 1),
            ('火山方舟 图片生成', 'volcengine', '将标准的图片生成请求转发到火山方舟官方 images 接口，body 保持 OpenAI 兼容格式', '{"target_type":"volcengine_image","path_rewrite":{"old":"/v1/images/generations","new":"/api/v3/images/generations"},"auth_type":"bearer"}', '图片', 1),
            ('火山方舟 视频素材转换', 'volcengine', '在火山方舟视频生成基础上，自动将 content 中的网络 URL 通过 CreateAsset API 转换为素材 ID（asset://前缀），需配置素材资产管理插件的审核凭证', '{"target_type":"volcengine","asset_convert":true,"path_rewrite":{"old":"/v1/video/generations","new":"/api/v3/contents/generations/tasks"},"auth_type":"bearer"}', '视频', 1),
            ('火山方舟 视频素材转换(国际版)', 'volcengine', '在火山方舟视频生成基础上，自动将 content 中的网络 URL 通过 CreateAsset API 转换为素材 ID（asset://前缀），需配置国际版素材资产管理插件的审核凭证', '{"target_type":"volcengine","asset_convert":true,"asset_convert_ns":"asset_manager_intl","path_rewrite":{"old":"/v1/video/generations","new":"/api/v3/contents/generations/tasks"},"auth_type":"bearer"}', '视频', 1),
            ('火山方舟 视频素材免审核转换(国际版)', 'volcengine', '在火山方舟视频生成基础上，自动将 content 中的网络 URL 通过 CreateAsset API 转换为素材 ID（asset://前缀），且向火山方舟申请免审核，需配置国际版素材资产管理插件的审核凭证', '{"target_type":"volcengine","asset_convert":true,"asset_convert_ns":"asset_manager_intl","moderation":true,"path_rewrite":{"old":"/v1/video/generations","new":"/api/v3/contents/generations/tasks"},"auth_type":"bearer"}', '视频', 1),
            ('阿里百炼 DashScope 视频生成', 'aliyun', '将标准视频生成请求（/v1/video/generations）转换为阿里百炼 DashScope 格式，支持文生视频/图生视频/参考生视频/视频编辑，异步任务自动注入 X-DashScope-Async Header', '{"target_type":"dashscope","path_rewrite":{"old":"/v1/video/generations","new":"/api/v1/services/aigc/video-generation/video-synthesis"},"auth_type":"bearer","poll_path":"/api/v1/tasks/${task_id}"}', '视频', 1),
            ('阿里百炼 DashScope 图片生成', 'aliyun', '将标准图片生成请求（/v1/images/generations）转换为阿里百炼 DashScope 格式', '{"target_type":"dashscope_image","path_rewrite":{"old":"/v1/images/generations","new":"/api/v1/services/aigc/multimodal-generation/generation"},"auth_type":"bearer"}', '图片', 1),
            ('阿里百炼 DashScope 聊天 (OpenAI兼容)', 'aliyun', '将标准聊天请求转发到阿里百炼兼容接口', '{"target_type":"openai","path_rewrite":{"old":"/v1/chat/completions","new":"/compatible-mode/v1/chat/completions"},"auth_type":"bearer"}', '聊天', 1),
            ('阿里百炼 DashScope 聊天 (Anthropic兼容)', 'aliyun', '将请求转换为 Anthropic 格式并转发到阿里百炼兼容接口', '{"target_type":"anthropic","path_rewrite":{"old":"/v1/messages","new":"/apps/anthropic/v1/messages"},"auth_type":"x-api-key"}', '聊天', 1),
            ('可灵 视频生成 (文/图/多图)', 'kling', '将标准视频生成请求转发到可灵官方 API，系统根据请求体自动分发到 text2video/image2video/multi-image2video', '{"target_type":"kling","path_rewrite":{"old":"/v1/video/generations","new":"/v1/videos/text2video"},"auth_type":"bearer"}', '视频', 1),
            ('可灵 Omni 视频 (kling-v3-omni/video-o1)', 'kling', '将视频生成请求转发到可灵 Omni 视频端点', '{"target_type":"kling","path_rewrite":{"old":"/v1/video/generations","new":"/v1/videos/omni-video"},"auth_type":"bearer"}', '视频', 1),
            ('可灵 图片生成', 'kling', '将标准图片生成请求转发到可灵官方 API，含多图参考自动分发', '{"target_type":"kling","path_rewrite":{"old":"/v1/images/generations","new":"/v1/images/generations"},"auth_type":"bearer"}', '图片', 1),
            ('可灵 Omni 图片 (kling-v3-omni/image-o1)', 'kling', '将图片生成请求转发到可灵 Omni 图片端点', '{"target_type":"kling","path_rewrite":{"old":"/v1/images/generations","new":"/v1/images/omni-image"},"auth_type":"bearer"}', '图片', 1),
            ('腾讯云 VOD AIGC 生图', 'tencent_vod', '将标准图片生成请求转换为腾讯云点播 AIGC CreateAigcImageTask 接口。密钥格式：SecretId:SecretKey:SubAppId，模型格式：ModelName@ModelVersion', '{"target_type":"tencent_vod_image","path_rewrite":{"old":"/v1/images/generations","new":"/"},"poll_path":"/v1/tasks/${task_id}","auth_type":"tencent_vod"}', '图片', 1),
            ('腾讯云 VOD AIGC 生图 (同步轮询)', 'tencent_vod', '同步版：无 poll_path，OpenAI 兼容请求将自动同步轮询至终态后返回结果。密钥格式：SecretId:SecretKey:SubAppId，模型格式：ModelName@ModelVersion', '{"target_type":"tencent_vod_image","path_rewrite":{"old":"/v1/images/generations","new":"/"},"auth_type":"tencent_vod"}', '图片', 1),
            ('腾讯云 VOD AIGC 生视频', 'tencent_vod', '将标准视频生成请求转换为腾讯云点播 AIGC CreateAigcVideoTask 接口。密钥格式：SecretId:SecretKey:SubAppId，模型格式：ModelName@ModelVersion', '{"target_type":"tencent_vod_video","path_rewrite":{"old":"/v1/video/generations","new":"/"},"auth_type":"tencent_vod"}', '视频', 1),
            ('即梦AI 图片生成', 'jimeng', '将标准图片生成请求转换为即梦AI（火山引擎 CV 视觉服务）格式。密钥格式：AccessKeyID:SecretAccessKey，模型映射为 req_key（如 high_aes_general_v30l_tta）', '{"target_type":"jimeng_image","path_rewrite":{"old":"/v1/images/generations","new":"/"},"auth_type":"jimeng"}', '图片', 1),
            ('即梦AI 视频生成', 'jimeng', '将标准视频生成请求转换为即梦AI（火山引擎 CV 视觉服务）格式。密钥格式：AccessKeyID:SecretAccessKey，模型映射为 req_key（如 dreamina_ic_generate_video_v2）', '{"target_type":"jimeng_video","path_rewrite":{"old":"/v1/video/generations","new":"/"},"auth_type":"jimeng"}', '视频', 1),
            ('GPT 官方图片生成', 'gpt', '将图片生成请求转发到 GPT 官方 API，自动根据请求体内容分发到 generations（文生图）或 edits（图生图/多图生图）端点', '{"target_type":"gpt","path_rewrite":{"old":"/v1/images/generations","new":"/v1/images/generations"},"auth_type":"bearer"}', '图片', 1),
            ('火山方舟 语音合成 (TTS V3)', 'volcengine', '将 OpenAI 格式语音合成请求（/v1/audio/speech）转换为火山方舟 TTS V3 SSE 格式。渠道地址: openspeech.bytedance.com，密钥为 X-Api-Key，模型ID通过 X-Api-Resource-Id 传递', '{"target_type":"volcengine_tts","path_rewrite":{"old":"/v1/audio/speech","new":"/api/v3/tts/unidirectional/sse"},"auth_type":"volcengine_tts"}', '音频', 1),
            ('火山方舟 语音合成 (TTS V3 Chunked)', 'volcengine', '将 OpenAI 格式语音合成请求（/v1/audio/speech）转换为火山方舟 TTS V3 HTTP Chunked 格式与 SSE 版本请求体和鉴权相同，仅传输协议不同（更轻量）', '{"target_type":"volcengine_tts","path_rewrite":{"old":"/v1/audio/speech","new":"/api/v3/tts/unidirectional"},"auth_type":"volcengine_tts"}', '音频', 1),
            ('OpenAI 兼容原生通道 (语音)', 'openai', '标准的语音合成透传规则，直接转发到 /v1/audio/speech', '{"path_rewrite":{"old":"/v1/audio/speech","new":"/v1/audio/speech"}}', '音频', 1),
            ('阿里百炼 DashScope 文本向量 (OpenAI兼容)', 'aliyun', '将文本向量请求转发到阿里百炼兼容接口', '{"target_type":"openai","path_rewrite":{"old":"/v1/embeddings","new":"/compatible-mode/v1/embeddings"},"auth_type":"bearer"}', '向量', 1),
            ('阿里百炼 DashScope 排序 (兼容模式)', 'aliyun', '将排序请求转发到阿里百炼兼容接口，适用于 qwen3-rerank 等模型', '{"target_type":"openai","path_rewrite":{"old":"/v1/rerank","new":"/compatible-api/v1/reranks"},"auth_type":"bearer"}', '排序', 1),
            ('阿里百炼 DashScope 排序 (原生)', 'aliyun', '将排序请求转发到阿里百炼原生 DashScope 接口，适用于 gte-rerank-v2 等模型', '{"target_type":"openai","path_rewrite":{"old":"/v1/rerank","new":"/api/v1/services/rerank/text-rerank/text-rerank"},"auth_type":"bearer"}', '排序', 1),
            ('Bytefor 视频生成', 'bytefor', '将标准的视频生成请求适配到 Bytefor 视频生成 API', '{"target_type":"bytefor_video","path_rewrite":{"old":"/v1/video/generations","new":"/api/v1/generate"},"poll_path":"/api/v1/task/${task_id}","auth_type":"bearer"}', '视频', 1),
            ('火山方舟 级联视频生成', 'volcengine', '供视频生成级联画质增强调用的火山方舟专属转发规则', '{"target_type":"volcengine","is_cascade":true,"res_mul":{"480p":1.5,"720p":2.15,"1080p":2.25,"2k":2.5,"4k":4.0},"path_rewrite":{"old":"/v1/video/generations","new":"/api/v3/contents/generations/tasks"},"auth_type":"bearer"}', '视频', 1),
            ('ATP Token 视频生成', 'atp', '将标准视频生成请求（/v1/video/generations）或阿里百炼格式参数转换为 ATP Token 媒体视频 API 格式（omni tasks），支持 Seedance / Kling / Wan / HappyHorse 系列模型，自动轮询及参数兼容', '{"target_type":"atp_video","path_rewrite":{"old":"/v1/video/generations","new":"/omni/media/v1/contents/generations/tasks"},"auth_type":"bearer","poll_path":"/omni/media/v1/contents/generations/tasks/${task_id}"}', '视频', 1)
        ) AS t(name, rule_type, description, config_json, category, is_system)
        WHERE NOT EXISTS (SELECT 1 FROM forward_rules WHERE name = t.name)
        "#,
        r#"CREATE TABLE IF NOT EXISTS billing_rules (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            billing_type TEXT NOT NULL,
            prompt_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            completion_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            cached_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            fixed_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            duration_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            billing_rule TEXT NOT NULL DEFAULT 'standard',
            pricing_tiers TEXT NOT NULL DEFAULT '[]',
            extended_config TEXT NOT NULL DEFAULT '{}',
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            upstream_type TEXT NOT NULL DEFAULT 'other',
            config TEXT,
            pid TEXT DEFAULT '',
            provider_id BIGINT REFERENCES model_providers(id),
            type_id BIGINT REFERENCES model_types(id),
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS billing_rule_id INTEGER",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS pre_deduction DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE models DROP COLUMN IF EXISTS billing_type",
        "ALTER TABLE models DROP COLUMN IF EXISTS prompt_rate",
        "ALTER TABLE models DROP COLUMN IF EXISTS completion_rate",
        "ALTER TABLE models DROP COLUMN IF EXISTS fixed_rate",
        "ALTER TABLE models DROP COLUMN IF EXISTS duration_rate",
        "ALTER TABLE models DROP COLUMN IF EXISTS billing_rule",
        "ALTER TABLE models DROP COLUMN IF EXISTS billing_unit",
        "ALTER TABLE models DROP COLUMN IF EXISTS pricing_tiers",
        "ALTER TABLE models DROP COLUMN IF EXISTS config",
        "ALTER TABLE models DROP COLUMN IF EXISTS upstream_type",
        "ALTER TABLE forward_rules DROP COLUMN IF EXISTS remark",
        "ALTER TABLE forward_rules DROP COLUMN IF EXISTS upstream_type",
        "ALTER TABLE forward_rules DROP COLUMN IF EXISTS config",
        "ALTER TABLE model_providers DROP COLUMN IF EXISTS upstream_type",
        "ALTER TABLE model_providers DROP COLUMN IF EXISTS config",
        "ALTER TABLE model_types DROP COLUMN IF EXISTS upstream_type",
        "ALTER TABLE model_types DROP COLUMN IF EXISTS config",
        "ALTER TABLE billing_rules DROP COLUMN IF EXISTS upstream_type",
        "ALTER TABLE billing_rules DROP COLUMN IF EXISTS config",
        "ALTER TABLE billing_rules DROP COLUMN IF EXISTS remark",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS extended_config TEXT NOT NULL DEFAULT '{}'",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS pid TEXT DEFAULT ''",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS provider_id BIGINT REFERENCES model_providers(id)",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS type_id BIGINT REFERENCES model_types(id)",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS is_system INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS cached_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS claude_cache_creation_rate DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS claude_cache_read_rate DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS pricing_type TEXT NOT NULL DEFAULT 'custom'",
        "UPDATE billing_rules SET name = '文本向量标准计费' WHERE name = '文本向量标准计费 (0.7/1M)'",
        "UPDATE billing_rules SET name = '排序模型多模态计费', billing_rule = 'multimodal', extended_config = '{\"image_prompt_rate\": 0.35}' WHERE name IN ('排序模型标准计费 (0.35/1M)', '排序模型标准计费')",
        r#"INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, extended_config, is_system)
        SELECT t.name, t.billing_type, t.prompt_rate, t.completion_rate, t.fixed_rate, t.duration_rate, t.billing_rule, t.extended_config, t.is_system
        FROM (VALUES 
            ('标准1M万字计费 (1)', 'tokens', 1.0, 2.0, 0.0, 0.0, 'standard', '{}', 1),
            ('单次请求扣费 (0.1)', 'requests', 0.0, 0.0, 0.1, 0.0, 'standard', '{}', 1),
            ('Seedance2.0官方计费', 'tokens', 0.0, 0.0, 0.0, 0.0, 'seedance2.0', '{"resolution_rates":{"1080p":{"with_video":31,"without_video":51},"480p":{"with_video":28,"without_video":46},"4k":{"with_video":16,"without_video":26},"720p":{"with_video":28,"without_video":46}}}', 1),
            ('Seedance2.0Fast官方计费', 'tokens', 0.0, 0.0, 0.0, 0.0, 'seedance2.0', '{"resolution_rates":{"480p":{"with_video":22,"without_video":37},"720p":{"with_video":22,"without_video":37}}}', 1),
            ('Seedance2.5官方计费', 'tokens', 0.0, 0.0, 0.0, 0.0, 'seedance2.0', '{"enable_time_multipliers":false,"resolution_rates":{"480p":{"with_video":42,"without_video":70},"720p":{"with_video":42,"without_video":70}},"time_multipliers":[]}', 1),
            ('可灵视频官方计费', 'duration', 0.0, 0.0, 0.0, 0.10, 'kling_video', '{"mode_multipliers":{"std":1.0,"pro":1.33,"4k":2.0},"sound_multipliers":{"off":1.0,"on":1.5}}', 1),
            ('可灵V3-Omni视频计费', 'duration', 0.0, 0.0, 0.0, 0.60, 'kling_video', '{"price_table":{"std|off|no":0.6,"std|on|no":0.8,"std|off|yes":0.9,"pro|off|no":0.8,"pro|on|no":1.0,"pro|off|yes":1.2,"4k|off|no":3.0,"4k|on|no":3.0,"4k|off|yes":3.0},"enable_mode":true,"enable_sound":true,"enable_video_ref":true}', 1),
            ('可灵Video-O1视频计费', 'duration', 0.0, 0.0, 0.0, 0.60, 'kling_video', '{"price_table":{"std|off|no":0.6,"std|off|yes":0.9,"pro|off|no":0.8,"pro|off|yes":1.2},"enable_mode":true,"enable_sound":false,"enable_video_ref":true}', 1),
            ('可灵V3视频计费', 'duration', 0.0, 0.0, 0.0, 0.60, 'kling_video', '{"price_table":{"std|off|no":0.6,"std|on|no":0.9,"pro|off|no":0.8,"pro|on|no":1.2,"4k|off|no":3.0,"4k|on|no":3.0},"enable_mode":true,"enable_sound":true,"enable_video_ref":false}', 1),
            ('语音合成按字符计费 (2.8元/万字符)', 'requests', 0.0, 0.0, 2.8, 0.0, 'characters', '{}', 1),
            ('文本向量标准计费', 'tokens', 0.7, 0.0, 0.0, 0.0, 'standard', '{}', 1),
            ('排序模型多模态计费', 'tokens', 0.35, 0.0, 0.0, 0.0, 'multimodal', '{"image_prompt_rate": 0.35}', 1),
            ('火山级联画质增强默认计费', 'duration', 0.0, 0.0, 0.0, 0.0, 'volc_enhance_cascade', '{"price_table": {"fast|720p|no": 0.80, "fast|1080p|no": 1.80, "fast|2k|no": 3.20, "fast|4k|no": 7.20, "standard|720p|no": 1.00, "standard|1080p|no": 2.24, "standard|2k|no": 4.00, "standard|4k|no": 8.94, "pro|720p|no": 1.20, "pro|1080p|no": 2.70, "pro|2k|no": 4.80, "pro|4k|no": 10.70, "ai|720p|no": 1.40, "ai|1080p|no": 3.16, "fast|720p|yes": 0.84, "fast|1080p|yes": 1.88, "fast|2k|yes": 3.40, "fast|4k|yes": 7.40, "standard|720p|yes": 1.06, "standard|1080p|yes": 2.36, "standard|2k|yes": 4.30, "standard|4k|yes": 9.24, "pro|720p|yes": 1.28, "pro|1080p|yes": 2.86, "pro|2k|yes": 5.20, "pro|4k|yes": 11.10, "ai|720p|yes": 1.60, "ai|1080p|yes": 3.51}}', 1)
        ) AS t(name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, extended_config, is_system)
        WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = t.name)
        "#,
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS register_ip TEXT DEFAULT ''",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS admin_remark TEXT DEFAULT ''",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS timezone TEXT DEFAULT 'Asia/Shanghai'",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS task_id TEXT DEFAULT ''",
        "COMMENT ON COLUMN logs.task_id IS '异步任务ID，非空时表示异步任务，用于轮询状态跟踪'",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS post_response TEXT DEFAULT ''",
        "COMMENT ON COLUMN logs.post_response IS '异步任务POST阶段提交响应结果'",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS action_type TEXT DEFAULT ''",
        "COMMENT ON COLUMN logs.action_type IS '任务类型：聊天、图片、视频等，用于精准筛选和显示'",
        "CREATE INDEX IF NOT EXISTS idx_logs_action_type_created ON logs (action_type, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_logs_task_id ON logs (task_id)",
        "CREATE INDEX IF NOT EXISTS idx_logs_token_created ON logs (token_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_logs_channel_created ON logs (channel_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_logs_model_created ON logs (model, created_at DESC)",
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS allow_view_log_details INTEGER NOT NULL DEFAULT 1",
        "COMMENT ON COLUMN user_levels.allow_view_log_details IS '是否允许查看日志详情，1-允许，0-不允许'",
        r#"CREATE TABLE IF NOT EXISTS channel_configs (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            provider_type TEXT NOT NULL,
            base_url TEXT NOT NULL,
            api_key TEXT NOT NULL,
            remark TEXT,
            yid TEXT DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS preset_id INTEGER",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS remark TEXT",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS yid TEXT DEFAULT ''",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE model_providers ADD COLUMN IF NOT EXISTS remark TEXT",
        "ALTER TABLE model_types ADD COLUMN IF NOT EXISTS remark TEXT",
        r#"CREATE TABLE IF NOT EXISTS upstreams (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            upstream_type TEXT NOT NULL DEFAULT 'other',
            sort_order INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            config TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE upstreams ADD COLUMN IF NOT EXISTS upstream_type TEXT NOT NULL DEFAULT 'other'",
        "ALTER TABLE upstreams ADD COLUMN IF NOT EXISTS config TEXT"
    );



    // Plugins table
    // ── 初始化插件管理系统表及种子数据（受一次性迁移保护） ──
    once_migration!(pool, "init_plugin_tables_v1",
        r#"CREATE TABLE IF NOT EXISTS plugins (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            description TEXT,
            is_enabled INTEGER NOT NULL DEFAULT 0,
            allowed_levels TEXT NOT NULL DEFAULT 'all',
            created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
        )"#,
        "ALTER TABLE plugins ADD COLUMN IF NOT EXISTS allowed_levels TEXT NOT NULL DEFAULT 'all'",
        r#"CREATE TABLE IF NOT EXISTS plugin_asset_groups (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            group_id TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            plugin_ns TEXT NOT NULL DEFAULT 'asset_manager',
            description TEXT,
            created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS plugin_assets (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            asset_type TEXT NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            file_name TEXT NOT NULL,
            file_url TEXT NOT NULL,
            mime_type TEXT,
            size INTEGER,
            reject_reason TEXT,
            category TEXT DEFAULT '未分类',
            asset_id TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            remark TEXT,
            group_id TEXT,
            plugin_ns TEXT NOT NULL DEFAULT 'asset_manager',
            created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
        )"#,
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS category TEXT DEFAULT '未分类'",
        "COMMENT ON COLUMN plugin_assets.category IS '素材分类'",
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS asset_id TEXT",
        "COMMENT ON COLUMN plugin_assets.asset_id IS '火山方舟素材ID（如 asset://...）'",
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN plugin_assets.sort_order IS '排序权重，数字越大越靠前'",
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS remark TEXT",
        "COMMENT ON COLUMN plugin_assets.remark IS '管理员内部备注'",
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS group_id TEXT",
        "COMMENT ON COLUMN plugin_assets.group_id IS '素材绑定的组合ID'",
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS content_hash TEXT",
        "COMMENT ON COLUMN plugin_assets.content_hash IS '资源内容 SHA-256 哈希值，用于精确去重'",
        "CREATE INDEX IF NOT EXISTS idx_plugin_assets_content_hash ON plugin_assets(content_hash)",
        "ALTER TABLE plugin_api_logs ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'page'",
        "COMMENT ON COLUMN plugin_api_logs.source IS '日志来源: api_proxy=对外接口 / page=页面操作 / relay_convert=转发规则替换素材'",
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS plugin_ns TEXT NOT NULL DEFAULT 'asset_manager'",
        "ALTER TABLE plugin_asset_groups ADD COLUMN IF NOT EXISTS plugin_ns TEXT NOT NULL DEFAULT 'asset_manager'",
        "CREATE INDEX IF NOT EXISTS idx_plugin_assets_asset_id_ns ON plugin_assets(asset_id, plugin_ns)",
        "CREATE INDEX IF NOT EXISTS idx_plugin_assets_source_ns ON plugin_assets(source, plugin_ns)",
        r#"INSERT INTO plugins (name, title, description, is_enabled)
           VALUES ('asset_manager', '素材资产管理', '提供全站图片、视频大模型使用的素材上传与审核功能', 0)
           ON CONFLICT (name) DO NOTHING"#,
        r#"INSERT INTO plugins (name, title, description, is_enabled)
           VALUES ('asset_manager_intl', '素材资产管理国际版', '提供全站图片、视频大模型使用的素材上传与审核功能（国际版）', 0)
           ON CONFLICT (name) DO NOTHING"#,
        r#"INSERT INTO plugins (name, title, description, is_enabled)
           VALUES ('team_marketing', '团队营销管理', '提供营销团队的用户管理，支持推广团队创建与成员管理', 0)
           ON CONFLICT (name) DO NOTHING"#,
        r#"INSERT INTO plugins (name, title, description, is_enabled)
           VALUES ('playground', '模型创作中心', '提供直接的视频、图片、声音、聊天模型体验服务', 0)
           ON CONFLICT (name) DO NOTHING"#
    );

    // ── 初始化内置服务商与模型创作中心表（受一次性迁移保护） ──
    once_migration!(pool, "init_playground_tables_v1",
        "ALTER TABLE model_providers ADD COLUMN IF NOT EXISTS is_system INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE model_types ADD COLUMN IF NOT EXISTS is_system INTEGER NOT NULL DEFAULT 0",
        "INSERT INTO model_providers (name, sort_order, is_system) VALUES ('火山引擎', 1, 1), ('谷歌', 2, 1), ('阿里云', 3, 1), ('腾讯云', 4, 1) ON CONFLICT(name) DO UPDATE SET is_system = 1",
        "INSERT INTO model_types (name, sort_order, is_system) VALUES ('视频', 1, 1), ('图片', 2, 1), ('音频', 3, 1), ('聊天', 4, 1), ('向量', 50, 1), ('排序', 60, 1), ('视频增强', 70, 1) ON CONFLICT(name) DO UPDATE SET is_system = 1",
        "UPDATE model_types SET default_features = '[\"输入-文字输入\",\"输入-语音输入\",\"输入-视频输入\",\"输出-文字输出\"]' WHERE name = '聊天' AND (default_features = '[]' OR default_features IS NULL)",
        "UPDATE model_types SET default_features = '[\"文生图\",\"图文生图\",\"图生图\"]' WHERE name = '图片' AND (default_features = '[]' OR default_features IS NULL)",
        "UPDATE model_types SET default_features = '[\"文生视频\",\"图生视频\",\"首尾帧生视频\",\"参考生视频\",\"视频生视频\"]' WHERE name = '视频' AND (default_features = '[]' OR default_features IS NULL)",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS mid TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS original_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS site_discount DOUBLE PRECISION NOT NULL DEFAULT 1.0",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS site_discount_enabled INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS global_discount DOUBLE PRECISION NOT NULL DEFAULT 1.0",
        "COMMENT ON COLUMN models.global_discount IS '全站折扣倍率，开启后与等级折扣/用户折扣取最小值'",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS global_discount_enabled INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN models.global_discount_enabled IS '全站折扣开关（0=关，1=开）'",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS model_id_alias TEXT NOT NULL DEFAULT ''",
        "COMMENT ON COLUMN models.model_id_alias IS '模型ID别名映射值，非空时上游请求使用此ID替代model_id（渠道映射优先级更高）'",
        r#"CREATE TABLE IF NOT EXISTS playground_projects (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            uid TEXT NOT NULL,
            name TEXT NOT NULL DEFAULT '未命名项目',
            description TEXT DEFAULT '',
            cover_url TEXT DEFAULT '',
            canvas_data TEXT DEFAULT '{}',
            is_deleted INTEGER NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_pg_projects_user ON playground_projects(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg_projects_uid ON playground_projects(uid)",
        r#"CREATE TABLE IF NOT EXISTS announcements (
            id SERIAL PRIMARY KEY,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS playground_assets (
            id SERIAL PRIMARY KEY,
            project_id INTEGER NOT NULL REFERENCES playground_projects(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            uid TEXT NOT NULL,
            asset_type TEXT NOT NULL,
            file_name TEXT DEFAULT '',
            file_size BIGINT DEFAULT 0,
            file_url TEXT NOT NULL,
            tos_object_key TEXT DEFAULT '',
            thumbnail_url TEXT DEFAULT '',
            prompt TEXT DEFAULT '',
            model_id TEXT DEFAULT '',
            model_name TEXT DEFAULT '',
            generation_params TEXT DEFAULT '{}',
            canvas_node_data TEXT DEFAULT '{}',
            duration_seconds DOUBLE PRECISION DEFAULT 0,
            width INTEGER DEFAULT 0,
            height INTEGER DEFAULT 0,
            is_deleted INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_pg_assets_project ON playground_assets(project_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg_assets_user ON playground_assets(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg_assets_type ON playground_assets(asset_type)",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS kid TEXT DEFAULT ''"
    );

    // Marketing Teams table
    // ── 初始化推广团队主表（受一次性迁移保护） ──
    once_migration!(pool, "init_marketing_teams_v1",
        r#"CREATE TABLE IF NOT EXISTS marketing_teams (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            invite_code TEXT UNIQUE,
            max_members INTEGER NOT NULL DEFAULT 10,
            created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
        )"#,
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS invite_code TEXT UNIQUE",
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS max_members INTEGER NOT NULL DEFAULT 10",
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS members_can_set_level INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS leader_can_remove_members INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN marketing_teams.leader_can_remove_members IS '团队负责人是否可以移除自己的推广成员(0=否,1=是)'"
    );

    // Backfill: generate invite_code for existing teams that don't have one (受一次性迁移保护)
    #[cfg(feature = "commercial_plugins")]
    {
        let invite_code_done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sys_migration_history WHERE id = 'backfill_team_invite_codes_v1'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        if invite_code_done == 0 {
            let teams_without_code: Vec<i64> = sqlx::query_scalar(
                "SELECT id FROM marketing_teams WHERE invite_code IS NULL OR invite_code = ''"
            ).fetch_all(&*pool).await.unwrap_or_default();
            for tid in teams_without_code {
                let code: String = (0..8).map(|_| {
                    let idx = rand::random::<u8>() % 36;
                    if idx < 10 { (b'0' + idx) as char } else { (b'a' + idx - 10) as char }
                }).collect();
                sqlx::query("UPDATE marketing_teams SET invite_code = $1 WHERE id = $2")
                    .bind(&code).bind(tid)
                    .execute(&*pool).await.ok();
            }
            let _ = sqlx::query("INSERT INTO sys_migration_history (id) VALUES ('backfill_team_invite_codes_v1')").execute(pool).await;
        }
    }

    // ── 初始化火山引擎卡池系统表及字段扩展（受一次性迁移保护） ──
    once_migration!(pool, "init_volcengine_pool_tables_v1",
        "ALTER TABLE plugins ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT 'user'",
        "COMMENT ON COLUMN plugins.category IS '插件分类: user=用户增强, system=系统增强'",
        "UPDATE plugins SET category = 'user' WHERE name IN ('asset_manager', 'asset_manager_intl', 'team_marketing', 'playground') AND category = ''",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('volcengine_pool', '火山引擎卡池系统', '管理多个火山引擎账号，实现智能调度、配额限制与故障自动隔离', 0, 'system')
           ON CONFLICT (name) DO NOTHING"#,
        r#"CREATE TABLE IF NOT EXISTS volcengine_pools (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            pool_type TEXT NOT NULL DEFAULT 'chat',
            strategy TEXT NOT NULL DEFAULT 'random',
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            model_id TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE volcengine_pools IS '火山引擎卡池分组表'",
        "COMMENT ON COLUMN volcengine_pools.pool_type IS '卡池类型: chat=聊天, image=图片, video=视频, custom=自定义'",
        "COMMENT ON COLUMN volcengine_pools.strategy IS '调度策略: random=随机分布, sequential=顺序轮转'",
        "ALTER TABLE volcengine_pools ADD COLUMN IF NOT EXISTS model_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS quota_unit",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS daily_reset_hour",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS daily_reset_minute",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS period_start",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS period_end",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS default_daily_quota",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS default_hourly_quota",
        "ALTER TABLE volcengine_pools DROP COLUMN IF EXISTS default_period_quota",
        r#"CREATE TABLE IF NOT EXISTS volcengine_pool_accounts (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            provider TEXT NOT NULL DEFAULT 'volcengine',
            base_url TEXT NOT NULL DEFAULT 'https://ark.cn-beijing.volces.com/api/v3',
            api_key TEXT NOT NULL,
            models TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL DEFAULT 'active',
            daily_quota DOUBLE PRECISION NOT NULL DEFAULT 0,
            hourly_quota DOUBLE PRECISION NOT NULL DEFAULT 0,
            period_quota DOUBLE PRECISION NOT NULL DEFAULT 0,
            daily_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            hourly_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            period_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            last_daily_reset TEXT NOT NULL DEFAULT '',
            last_hourly_reset TEXT NOT NULL DEFAULT '',
            last_period_reset TEXT NOT NULL DEFAULT '',
            last_error TEXT,
            last_error_at TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS provider TEXT NOT NULL DEFAULT 'volcengine'",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS base_url TEXT NOT NULL DEFAULT 'https://ark.cn-beijing.volces.com/api/v3'",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS daily_quota DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS hourly_quota DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS period_quota DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS daily_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS hourly_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS period_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS last_daily_reset TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS last_hourly_reset TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS last_period_reset TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS last_error TEXT",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS last_error_at TEXT",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS daily_reset_hour INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS daily_reset_minute INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS period_start TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_accounts ADD COLUMN IF NOT EXISTS period_end TEXT NOT NULL DEFAULT ''",
        "COMMENT ON TABLE volcengine_pool_accounts IS '火山引擎独立账号表'",
        "COMMENT ON COLUMN volcengine_pool_accounts.base_url IS '请求地址'",
        "COMMENT ON COLUMN volcengine_pool_accounts.models IS '支持的模型列表，逗号分隔'",
        "COMMENT ON COLUMN volcengine_pool_accounts.status IS '账号状态: active=可用, disabled=故障禁用, exhausted=配额耗尽'",
        "COMMENT ON COLUMN volcengine_pool_accounts.daily_quota IS '每日配额上限(0=不限)'",
        "COMMENT ON COLUMN volcengine_pool_accounts.hourly_quota IS '每小时配额上限(0=不限)'",
        "COMMENT ON COLUMN volcengine_pool_accounts.period_quota IS '时段配额上限(0=不限)'",
        r#"CREATE TABLE IF NOT EXISTS volcengine_pool_account_mapping (
            pool_id INTEGER NOT NULL REFERENCES volcengine_pools(id) ON DELETE CASCADE,
            account_id INTEGER NOT NULL REFERENCES volcengine_pool_accounts(id) ON DELETE CASCADE,
            PRIMARY KEY (pool_id, account_id)
        )"#,
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active'",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS quota_unit TEXT NOT NULL DEFAULT 'tokens'",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS daily_reset_hour INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS daily_reset_minute INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS period_start TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS period_end TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS daily_quota DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS hourly_quota DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS period_quota DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS daily_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS hourly_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS period_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS last_daily_reset TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS last_hourly_reset TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS last_period_reset TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE volcengine_pool_account_mapping ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON TABLE volcengine_pool_account_mapping IS '卡池与账号的多对多映射表'",
        r#"CREATE TABLE IF NOT EXISTS volcengine_pool_logs (
            id SERIAL PRIMARY KEY,
            pool_id INTEGER NOT NULL,
            account_id INTEGER NOT NULL,
            account_name TEXT NOT NULL DEFAULT '',
            model_id TEXT NOT NULL DEFAULT '',
            channel_id INTEGER NOT NULL DEFAULT 0,
            usage_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
            quota_unit TEXT NOT NULL DEFAULT 'tokens',
            status TEXT NOT NULL DEFAULT 'success',
            error_message TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE volcengine_pool_logs IS '卡池调度使用日志'",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS pool_id INTEGER",
        "COMMENT ON COLUMN channels.pool_id IS '关联的火山引擎卡池ID，为空表示不使用卡池'",
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS allowed_level_ids TEXT NOT NULL DEFAULT '[]'",
        "COMMENT ON COLUMN marketing_teams.allowed_level_ids IS '团队负责人被授权可分配的用户等级ID列表(JSON数组)'",
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS allowed_member_level_ids TEXT NOT NULL DEFAULT '[]'",
        "COMMENT ON COLUMN marketing_teams.allowed_member_level_ids IS '团队负责人被授权可分配给团队成员的用户等级ID列表(JSON数组)'"
    );

    // Marketing Team Leaders table (many-to-many)
    // ── 初始化推广团队关联表与公共配置表（受一次性迁移保护） ──
    once_migration!(pool, "init_marketing_team_relations_v1",
        r#"CREATE TABLE IF NOT EXISTS marketing_team_leaders (
            id SERIAL PRIMARY KEY,
            team_id INTEGER NOT NULL,
            user_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            UNIQUE(team_id, user_id)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS marketing_team_members (
            id SERIAL PRIMARY KEY,
            team_id INTEGER NOT NULL,
            user_id TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            UNIQUE(team_id, user_id)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS plugin_configs (
            id SERIAL PRIMARY KEY,
            plugin_name TEXT NOT NULL,
            config_key TEXT NOT NULL,
            config_value TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
            UNIQUE(plugin_name, config_key)
        )"#
    );

    // -- 多方式登录注册扩展 --
    // users 表新增 google_id（谷歌 OAuth 唯一标识）与微信登录字段，受一次性迁移保护
    once_migration!(pool, "user_oauth_fields_v1",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS google_id TEXT",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS wechat_name TEXT",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS google_name TEXT",
        "ALTER TABLE verification_codes ADD COLUMN IF NOT EXISTS phone TEXT DEFAULT ''"
    );


    // 回填已有令牌的 kid（只处理 kid 为空的记录，受一次性迁移保护）
    {
        let kid_backfill_done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sys_migration_history WHERE id = 'backfill_token_kids_v1'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        if kid_backfill_done == 0 {
            #[derive(sqlx::FromRow)]
            struct TokenUid { token_id: i64, uid: String }
            let rows: Vec<TokenUid> = sqlx::query_as(
                "SELECT t.id as token_id, u.uid FROM api_tokens t JOIN users u ON t.user_id = u.id WHERE t.kid IS NULL OR t.kid = ''"
            ).fetch_all(&*pool).await.unwrap_or_default();
            for row in rows {
                let uid_suffix: String = row.uid.chars().rev().take(3).collect::<String>().chars().rev().collect();
                let random_part: String = (0..3).map(|_| (b'0' + rand::random::<u8>() % 10) as char).collect();
                let kid = format!("{}{}", uid_suffix, random_part);
                sqlx::query("UPDATE api_tokens SET kid = $1 WHERE id = $2")
                    .bind(&kid).bind(row.token_id)
                    .execute(&*pool).await.ok();
            }
            let _ = sqlx::query("INSERT INTO sys_migration_history (id) VALUES ('backfill_token_kids_v1')").execute(pool).await;
        }
    }


    // ── 初始化 GPT-Image 卡池系统表及字段配置（受一次性迁移保护） ──
    once_migration!(pool, "init_gptimage_pool_tables_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('gptimage_pool', 'GPT-Image卡池系统', '管理多个GPT-Image来源账号，实现智能调度、配额限制与故障自动隔离', 0, 'system')
           ON CONFLICT (name) DO NOTHING"#,
        r#"CREATE TABLE IF NOT EXISTS gptimage_pools (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            pool_type TEXT NOT NULL DEFAULT 'image',
            strategy TEXT NOT NULL DEFAULT 'random',
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE gptimage_pools IS 'GPT-Image卡池分组表'",
        "COMMENT ON COLUMN gptimage_pools.pool_type IS '卡池类型: image=图片, custom=自定义'",
        "COMMENT ON COLUMN gptimage_pools.strategy IS '调度策略: random=随机分布, sequential=顺序轮转'",
        r#"CREATE TABLE IF NOT EXISTS gptimage_pool_accounts (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            base_url TEXT NOT NULL DEFAULT '',
            api_key TEXT NOT NULL,
            models TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',
            quota_unit TEXT NOT NULL DEFAULT 'images',
            daily_reset_hour INTEGER NOT NULL DEFAULT 0,
            daily_reset_minute INTEGER NOT NULL DEFAULT 0,
            period_start TEXT NOT NULL DEFAULT '',
            period_end TEXT NOT NULL DEFAULT '',
            daily_quota DOUBLE PRECISION NOT NULL DEFAULT 0,
            hourly_quota DOUBLE PRECISION NOT NULL DEFAULT 0,
            period_quota DOUBLE PRECISION NOT NULL DEFAULT 0,
            daily_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            hourly_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            period_used DOUBLE PRECISION NOT NULL DEFAULT 0,
            last_daily_reset TEXT NOT NULL DEFAULT '',
            last_hourly_reset TEXT NOT NULL DEFAULT '',
            last_period_reset TEXT NOT NULL DEFAULT '',
            last_error TEXT,
            last_error_at TEXT,
            priority INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE gptimage_pool_accounts IS 'GPT-Image来源账号表'",
        "COMMENT ON COLUMN gptimage_pool_accounts.base_url IS '请求地址，如 https://api.openai.com'",
        "COMMENT ON COLUMN gptimage_pool_accounts.models IS '支持的模型列表，逗号分隔'",
        "COMMENT ON COLUMN gptimage_pool_accounts.quota_unit IS '配额计量单位: tokens=Token数, requests=请求次数, images=图片张数'",
        "COMMENT ON COLUMN gptimage_pool_accounts.status IS '账号状态: active=可用, disabled=故障禁用, exhausted=配额耗尽'",
        "COMMENT ON COLUMN gptimage_pool_accounts.daily_quota IS '每日配额上限(0=不限)'",
        "COMMENT ON COLUMN gptimage_pool_accounts.hourly_quota IS '每小时配额上限(0=不限)'",
        "COMMENT ON COLUMN gptimage_pool_accounts.period_quota IS '时段配额上限(0=不限)'",
        r#"CREATE TABLE IF NOT EXISTS gptimage_pool_account_mapping (
            pool_id INTEGER NOT NULL REFERENCES gptimage_pools(id) ON DELETE CASCADE,
            account_id INTEGER NOT NULL REFERENCES gptimage_pool_accounts(id) ON DELETE CASCADE,
            PRIMARY KEY (pool_id, account_id)
        )"#,
        "COMMENT ON TABLE gptimage_pool_account_mapping IS 'GPT-Image卡池与账号的多对多映射表'",
        r#"CREATE TABLE IF NOT EXISTS gptimage_pool_logs (
            id SERIAL PRIMARY KEY,
            pool_id INTEGER NOT NULL,
            account_id INTEGER NOT NULL,
            account_name TEXT NOT NULL DEFAULT '',
            model_id TEXT NOT NULL DEFAULT '',
            channel_id INTEGER NOT NULL DEFAULT 0,
            usage_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
            quota_unit TEXT NOT NULL DEFAULT 'images',
            status TEXT NOT NULL DEFAULT 'success',
            error_message TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE gptimage_pool_logs IS 'GPT-Image卡池调度使用日志'",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS gptimage_pool_id INTEGER",
        "COMMENT ON COLUMN channels.gptimage_pool_id IS '关联的GPT-Image卡池ID，为空表示不使用卡池'",
        "ALTER TABLE models DROP CONSTRAINT IF EXISTS models_name_key",
        "ALTER TABLE models DROP CONSTRAINT IF EXISTS models_model_id_key"
    );

    // ══════════════════════════════════════════════════════════════
    //  模型广场管理插件
    // ══════════════════════════════════════════════════════════════
    // ── 初始化站点图标库及流转计费日志扩展（受一次性迁移保护） ──
    once_migration!(pool, "init_site_icons_tables_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('model_marketplace', '模型广场管理', '管理模型广场的模型展示，控制哪些模型对用户可见并配置展示信息', 0, 'user')
           ON CONFLICT (name) DO NOTHING"#,
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('site_icons', '站点icon图标库', '提供 AI/LLM 品牌 SVG 图标库，支持搜索选择和自定义上传，数据来源 lobehub/lobe-icons', 1, 'system_builtin')
           ON CONFLICT (name) DO NOTHING"#,
        r#"CREATE TABLE IF NOT EXISTS site_icons (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            file_path TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL DEFAULT 'lobe-icons',
            category TEXT NOT NULL DEFAULT 'AI品牌',
            tags TEXT NOT NULL DEFAULT '[]',
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text),
            UNIQUE(name, source)
        )"#,
        "COMMENT ON TABLE site_icons IS '站点图标库，存储 SVG 图标元数据'",
        "COMMENT ON COLUMN site_icons.name IS '图标标识名（如 openai, claude）'",
        "COMMENT ON COLUMN site_icons.title IS '显示名称（如 OpenAI, Claude）'",
        "COMMENT ON COLUMN site_icons.file_path IS 'SVG 文件路径（相对于 data/assets/）'",
        "COMMENT ON COLUMN site_icons.source IS '图标来源: lobe-icons=从 GitHub 同步 / custom=手动上传'",
        "COMMENT ON COLUMN site_icons.category IS '分类: AI品牌 / 自定义'",
        "COMMENT ON COLUMN site_icons.tags IS '标签(JSON数组)'",
        r#"CREATE TABLE IF NOT EXISTS site_icon_sync_logs (
            id SERIAL PRIMARY KEY,
            total_synced INTEGER NOT NULL DEFAULT 0,
            total_new INTEGER NOT NULL DEFAULT 0,
            total_updated INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'success',
            error_message TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE site_icon_sync_logs IS '站点图标同步日志'",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS logo TEXT",
        "ALTER TABLE model_providers ADD COLUMN IF NOT EXISTS logo TEXT",
        "ALTER TABLE model_types ADD COLUMN IF NOT EXISTS logo TEXT",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS cached_tokens INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN logs.cached_tokens IS '缓存命中的Token数量(属于输入的子集)'",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS billing_features TEXT",
        "COMMENT ON COLUMN logs.billing_features IS 'POST阶段提取的计费特征快照(JSON)，独立于enable_log开关，确保异步任务结算时始终有完整计费参数'",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS pre_deduct_gift DOUBLE PRECISION NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN logs.pre_deduct_gift IS '预扣费中从赠送余额扣除的金额，用于退款时精准归还到对应钱包'",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS remark TEXT",
        "COMMENT ON COLUMN users.remark IS '推广用户备注'",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS referral_history TEXT DEFAULT ''",
        "COMMENT ON COLUMN users.referral_history IS '关联流转记录'",
        "INSERT INTO model_providers (name, sort_order, is_system) VALUES ('可灵 AI', 4, 1) ON CONFLICT(name) DO UPDATE SET is_system = 1"
    );

    // ══════════════════════════════════════════════════════════════
    //  智能路由 (Router Flow) 插件
    // ══════════════════════════════════════════════════════════════
    // ── 初始化智能路由表及通道等级字段扩充（受一次性迁移保护） ──
    once_migration!(pool, "init_router_flow_tables_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('router_flow', '智能路由', '配置多个相同模型组成高可用路由组，支持价格优先、速度优先、稳定优先三种智能调度策略', 0, 'user')
           ON CONFLICT (name) DO NOTHING"#,
        r#"CREATE TABLE IF NOT EXISTS router_flow_groups (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            description TEXT DEFAULT '',
            route_rule TEXT NOT NULL DEFAULT 'price',
            model_ids TEXT NOT NULL DEFAULT '[]',
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE router_flow_groups IS '智能路由模型组表，用户创建的模型路由组'",
        "COMMENT ON COLUMN router_flow_groups.route_rule IS '路由策略: price=价格优先, speed=速度优先, stability=稳定优先'",
        "COMMENT ON COLUMN router_flow_groups.model_ids IS '绑定的模型 mid 列表(JSON数组)'",
        "CREATE INDEX IF NOT EXISTS idx_rf_groups_user ON router_flow_groups(user_id)",
        "ALTER TABLE router_flow_groups ADD COLUMN IF NOT EXISTS endpoint_id TEXT NOT NULL DEFAULT ''",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_rf_groups_endpoint ON router_flow_groups(endpoint_id) WHERE endpoint_id != ''",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS exclude_user_groups TEXT NOT NULL DEFAULT '[]'",
        "COMMENT ON COLUMN channels.exclude_user_groups IS '不支持的用户等级列表(JSON数组)，黑名单模式'",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS gift_balance DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS gift_used_quota DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "COMMENT ON COLUMN users.gift_balance IS '赠送钱包余额，注册赠送/活动赠送等免费额度，消费时优先扣赠送余额'",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS last_used_at VARCHAR(30) DEFAULT NULL",
        "COMMENT ON COLUMN api_tokens.last_used_at IS '令牌最后使用时间'"
    );
    // ─── 统一将所有 INTEGER 列升级为 BIGINT，与 Rust 模型层 i64 对齐 ───
    once_migration!(pool, "upgrade_columns_to_bigint_v1",

        // ── user_levels ──
        "ALTER TABLE user_levels ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE user_levels ALTER COLUMN daily_invite_limit TYPE BIGINT",
        "ALTER TABLE user_levels ALTER COLUMN marketing_enabled TYPE BIGINT",
        "ALTER TABLE user_levels ALTER COLUMN is_default TYPE BIGINT",
        "ALTER TABLE user_levels ALTER COLUMN max_token_count TYPE BIGINT",
        // ── users ──
        "ALTER TABLE users ALTER COLUMN is_active TYPE BIGINT",
        "ALTER TABLE users ALTER COLUMN admin_group_id TYPE BIGINT",
        // ── api_tokens ──
        "ALTER TABLE api_tokens ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE api_tokens ALTER COLUMN is_active TYPE BIGINT",
        // ── channels ──
        "ALTER TABLE channels ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE channels ALTER COLUMN preset_id TYPE BIGINT",
        "ALTER TABLE channels ALTER COLUMN pool_id TYPE BIGINT",
        "ALTER TABLE channels ALTER COLUMN gptimage_pool_id TYPE BIGINT",
        // ── logs ──
        "ALTER TABLE logs ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE logs ALTER COLUMN channel_id TYPE BIGINT",
        "ALTER TABLE logs ALTER COLUMN token_id TYPE BIGINT",
        // ── channel_configs ──
        "ALTER TABLE channel_configs ALTER COLUMN id TYPE BIGINT",
        // ── admin_groups ──
        "ALTER TABLE admin_groups ALTER COLUMN id TYPE BIGINT",
        // ── plugins ──
        "ALTER TABLE plugins ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE plugins ALTER COLUMN is_enabled TYPE BIGINT",
        // ── site_icons ──
        "ALTER TABLE site_icons ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE site_icons ALTER COLUMN is_active TYPE BIGINT",
        // ── site_icon_sync_logs ──
        "ALTER TABLE site_icon_sync_logs ALTER COLUMN total_synced TYPE BIGINT",
        "ALTER TABLE site_icon_sync_logs ALTER COLUMN total_new TYPE BIGINT",
        "ALTER TABLE site_icon_sync_logs ALTER COLUMN total_updated TYPE BIGINT",
        // ── redemptions（原误写为 redemption_codes，已修正） ──
        "ALTER TABLE redemptions ALTER COLUMN id TYPE BIGINT",
        // ── models ──
        "ALTER TABLE models ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE models ALTER COLUMN provider_id TYPE BIGINT",
        "ALTER TABLE models ALTER COLUMN type_id TYPE BIGINT",
        "ALTER TABLE models ALTER COLUMN billing_rule_id TYPE BIGINT",
        // ── model_providers ──
        "ALTER TABLE model_providers ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE model_providers ADD COLUMN IF NOT EXISTS name_en TEXT NOT NULL DEFAULT ''",
        // ── model_types ──
        "ALTER TABLE model_types ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE model_types ADD COLUMN IF NOT EXISTS name_en TEXT NOT NULL DEFAULT ''",
        // ── forward_rules ──
        "ALTER TABLE forward_rules ALTER COLUMN id TYPE BIGINT",
        // ── billing_rules ──
        "ALTER TABLE billing_rules ALTER COLUMN id TYPE BIGINT",
        // ── recharge_records ──
        "ALTER TABLE recharge_records ALTER COLUMN id TYPE BIGINT",
        // ── orders ──
        "ALTER TABLE orders ALTER COLUMN id TYPE BIGINT",
        // ── upstreams ──
        "ALTER TABLE upstreams ALTER COLUMN id TYPE BIGINT",
        // ── announcements ──
        "ALTER TABLE announcements ALTER COLUMN id TYPE BIGINT",
        // ── verification_codes ──
        "ALTER TABLE verification_codes ALTER COLUMN id TYPE BIGINT",
        // ── volcengine_pools（主表） ──
        "ALTER TABLE volcengine_pools ALTER COLUMN id TYPE BIGINT",
        // ── volcengine_pool_accounts ──
        "ALTER TABLE volcengine_pool_accounts ALTER COLUMN id TYPE BIGINT",
        // ── volcengine_pool_account_mapping ──
        "ALTER TABLE volcengine_pool_account_mapping ALTER COLUMN pool_id TYPE BIGINT",
        "ALTER TABLE volcengine_pool_account_mapping ALTER COLUMN account_id TYPE BIGINT",
        // ── volcengine_pool_logs ──
        "ALTER TABLE volcengine_pool_logs ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE volcengine_pool_logs ALTER COLUMN pool_id TYPE BIGINT",
        "ALTER TABLE volcengine_pool_logs ALTER COLUMN account_id TYPE BIGINT",
        "ALTER TABLE volcengine_pool_logs ALTER COLUMN channel_id TYPE BIGINT",
        // ── gptimage_pools（主表） ──
        "ALTER TABLE gptimage_pools ALTER COLUMN id TYPE BIGINT",
        // ── gptimage_pool_accounts ──
        "ALTER TABLE gptimage_pool_accounts ALTER COLUMN id TYPE BIGINT",
        // ── gptimage_pool_account_mapping ──
        "ALTER TABLE gptimage_pool_account_mapping ALTER COLUMN pool_id TYPE BIGINT",
        "ALTER TABLE gptimage_pool_account_mapping ALTER COLUMN account_id TYPE BIGINT",
        // ── gptimage_pool_logs ──
        "ALTER TABLE gptimage_pool_logs ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE gptimage_pool_logs ALTER COLUMN pool_id TYPE BIGINT",
        "ALTER TABLE gptimage_pool_logs ALTER COLUMN account_id TYPE BIGINT",
        "ALTER TABLE gptimage_pool_logs ALTER COLUMN channel_id TYPE BIGINT",
        // ── playground_projects（体验中心项目） ──
        "ALTER TABLE playground_projects ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE playground_projects ALTER COLUMN is_deleted TYPE BIGINT",
        // ── playground_assets（体验中心资源） ──
        "ALTER TABLE playground_assets ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE playground_assets ALTER COLUMN project_id TYPE BIGINT",
        "ALTER TABLE playground_assets ALTER COLUMN width TYPE BIGINT",
        "ALTER TABLE playground_assets ALTER COLUMN height TYPE BIGINT",
        "ALTER TABLE playground_assets ALTER COLUMN is_deleted TYPE BIGINT",
        // ── plugin_api_logs ──
        "ALTER TABLE plugin_api_logs ALTER COLUMN id TYPE BIGINT",
        // ── plugin_configs ──
        "ALTER TABLE plugin_configs ALTER COLUMN id TYPE BIGINT",
        // ── plugin_asset_groups ──
        "ALTER TABLE plugin_asset_groups ALTER COLUMN id TYPE BIGINT",
        // ── plugin_assets ──
        "ALTER TABLE plugin_assets ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE plugin_assets ALTER COLUMN size TYPE BIGINT",
        "ALTER TABLE plugin_assets ALTER COLUMN sort_order TYPE BIGINT",
        // ── marketing_teams ──
        "ALTER TABLE marketing_teams ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE marketing_teams ALTER COLUMN max_members TYPE BIGINT",
        "ALTER TABLE marketing_teams ALTER COLUMN members_can_set_level TYPE BIGINT",
        "ALTER TABLE marketing_teams ALTER COLUMN leader_can_remove_members TYPE BIGINT",
        // ── marketing_team_leaders ──
        "ALTER TABLE marketing_team_leaders ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE marketing_team_leaders ALTER COLUMN team_id TYPE BIGINT",
        // ── marketing_team_members ──
        "ALTER TABLE marketing_team_members ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE marketing_team_members ALTER COLUMN team_id TYPE BIGINT",
        // ── site_icon_sync_logs ──
        "ALTER TABLE site_icon_sync_logs ALTER COLUMN id TYPE BIGINT",
        // ── router_flow_groups ──
        "ALTER TABLE router_flow_groups ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE router_flow_groups ALTER COLUMN is_active TYPE BIGINT",

    );

    // recharge_records 新增 operator 字段（记录后台操作人员）和 wallet_type 字段（区分所属钱包），受一次性迁移保护
    once_migration!(pool, "recharge_records_wallet_fields_v1",
        "ALTER TABLE recharge_records ADD COLUMN IF NOT EXISTS operator TEXT DEFAULT ''",
        "COMMENT ON COLUMN recharge_records.operator IS '操作人员用户名，后台手动操作时记录'",
        "ALTER TABLE recharge_records ADD COLUMN IF NOT EXISTS wallet_type TEXT NOT NULL DEFAULT 'system'",
        "COMMENT ON COLUMN recharge_records.wallet_type IS '所属钱包类型: system=系统钱包, gift=赠送钱包'"
    );



    // 迁移历史数据（受一次性迁移保护，仅执行一次，避免大表全扫描引起卡顿）
    // 1. 原 recharge_type='gift' 的记录归入赠送钱包
    // 2. 补充修复：registration（注册赠送）和 commission（邀请奖励）类型实际写入赠送钱包，
    //    但早期未正确设置 wallet_type，导致赠送钱包明细为空但余额不为零的数据不一致
    once_migration!(pool, "backfill_recharge_wallet_type_v1",
        "UPDATE recharge_records SET wallet_type = 'gift' WHERE recharge_type = 'gift' AND wallet_type = 'system'",
        "UPDATE recharge_records SET wallet_type = 'gift' WHERE recharge_type IN ('registration', 'commission') AND wallet_type = 'system'"
    );

    // ══════════════════════════════════════════════════════════════
    //  API服务商 (API Providers) 支持
    // ══════════════════════════════════════════════════════════════
    // ── 初始化API服务商与临时文件折扣锁定参数等表结构（受一次性迁移保护） ──
    once_migration!(pool, "init_api_providers_tables_v1",
        r#"CREATE TABLE IF NOT EXISTS model_api_providers (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            name_en TEXT NOT NULL DEFAULT '',
            sort_order INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            is_system INTEGER NOT NULL DEFAULT 0,
            remark TEXT,
            logo TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "COMMENT ON TABLE model_api_providers IS 'API服务商表（提供接口的服务商，区别于官方服务商）'",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS api_provider_id BIGINT REFERENCES model_api_providers(id)",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('site_portal', '站点门户', '提供站点内容的基本介绍，支持生成静态HTML页面用于SEO/GEO优化', 0, 'user')
           ON CONFLICT (name) DO NOTHING"#,
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS only_playground BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE api_tokens ALTER COLUMN only_playground TYPE BIGINT",
        "COMMENT ON COLUMN api_tokens.only_playground IS '是否仅限创作中心使用，1=是，0=否'",
        r#"CREATE TABLE IF NOT EXISTS tos_temp_files (
            id SERIAL PRIMARY KEY,
            object_key TEXT NOT NULL,
            channel_id INTEGER NOT NULL DEFAULT 0,
            source TEXT NOT NULL DEFAULT 'channel',
            expire_at TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_tos_temp_files_expire ON tos_temp_files (expire_at)",
        "ALTER TABLE tos_temp_files ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE tos_temp_files ALTER COLUMN channel_id TYPE BIGINT",
        "COMMENT ON TABLE tos_temp_files IS 'TOS临时文件过期追踪'",
        "COMMENT ON COLUMN tos_temp_files.object_key IS 'TOS对象键'",
        "COMMENT ON COLUMN tos_temp_files.channel_id IS '来源渠道ID'",
        "COMMENT ON COLUMN tos_temp_files.source IS '业务来源(channel=渠道存储)'",
        "COMMENT ON COLUMN tos_temp_files.expire_at IS '过期时间(ISO 8601)'",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS model_discounts TEXT",
        "COMMENT ON COLUMN users.model_discounts IS '用户模型单独折扣(JSON: {mid: discount}), 优先于等级折扣, 受模型折扣限价约束'",
        r#"CREATE TABLE IF NOT EXISTS user_model_configs (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            model_mid TEXT NOT NULL,
            param_values TEXT NOT NULL DEFAULT '{}',
            is_locked INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text),
            UNIQUE(user_id, model_mid)
        )"#,
        "ALTER TABLE user_model_configs ALTER COLUMN id TYPE bigint",
        "CREATE INDEX IF NOT EXISTS idx_user_model_configs_user ON user_model_configs(user_id)",
        "COMMENT ON TABLE user_model_configs IS '用户在模型创作中心锁定的模型自定义参数配置'",
        "COMMENT ON COLUMN user_model_configs.user_id IS '用户ID'",
        "COMMENT ON COLUMN user_model_configs.model_mid IS '模型MID标识'",
        "COMMENT ON COLUMN user_model_configs.param_values IS '锁定的配置参数序列化JSON串'",
        "COMMENT ON COLUMN user_model_configs.is_locked IS '是否已锁定，1=是，0=否'",
        r#"CREATE TABLE IF NOT EXISTS happyhorse_logs (
            id SERIAL PRIMARY KEY,
            user_id TEXT NOT NULL,
            original_model TEXT NOT NULL,
            media_type TEXT NOT NULL,
            matched_model TEXT NOT NULL,
            status INTEGER NOT NULL,
            latency_ms INTEGER NOT NULL DEFAULT 0,
            error_message TEXT,
            task_id TEXT,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_happyhorse_logs_created ON happyhorse_logs (created_at DESC)",
        "COMMENT ON TABLE happyhorse_logs IS '快乐小马智能路由转换日志表'",
        "COMMENT ON COLUMN happyhorse_logs.original_model IS '原始请求模型ID'",
        "COMMENT ON COLUMN happyhorse_logs.media_type IS '媒体类型(文生视频/图生视频/参考生视频/视频编辑)'",
        "COMMENT ON COLUMN happyhorse_logs.matched_model IS '路由分发的实际模型ID'",
        "ALTER TABLE happyhorse_logs ADD COLUMN IF NOT EXISTS log_id BIGINT",
        "COMMENT ON COLUMN happyhorse_logs.log_id IS '关联主日志表logs.id，用于JOIN获取完整请求/响应/计费信息'",
        "CREATE INDEX IF NOT EXISTS idx_happyhorse_logs_log_id ON happyhorse_logs (log_id)",
        "ALTER TABLE happyhorse_logs DROP COLUMN IF EXISTS request_payload",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS plugin_tag TEXT DEFAULT ''",
        "COMMENT ON COLUMN logs.plugin_tag IS '插件标记JSON，用于匹配规则展示和插件解耦'"
    );
    // happyhorse_logs: user_id → user_uid（存储短标识，提高效率和可读性）
    // 注意：新功能表存储用户标识统一使用 uid（users.uid）而非 user_id（users.id）
    // 使用 sys_migration_history 一次性机制包裹，并优先检查列是否存在，避免重复运行报错
    let rename_user_id_done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sys_migration_history WHERE id = 'happyhorse_logs_rename_user_id_to_user_uid'")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if rename_user_id_done == 0 {
        let col_exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM information_schema.columns WHERE table_name = 'happyhorse_logs' AND column_name = 'user_id'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        if col_exists > 0 {
            let _ = sqlx::query("ALTER TABLE happyhorse_logs RENAME COLUMN user_id TO user_uid").execute(pool).await;
            let _ = sqlx::query("UPDATE happyhorse_logs SET user_uid = COALESCE((SELECT u.uid FROM users u WHERE u.id = happyhorse_logs.user_uid), user_uid)").execute(pool).await;
            let _ = sqlx::query("COMMENT ON COLUMN happyhorse_logs.user_uid IS '用户短标识(users.uid)'").execute(pool).await;
        }
        let _ = sqlx::query("INSERT INTO sys_migration_history (id) VALUES ('happyhorse_logs_rename_user_id_to_user_uid')").execute(pool).await;
    }

    // ── 初始化快乐小马智能路由系统配置及种子数据（受一次性迁移保护） ──
    once_migration!(pool, "init_happyhorse_router_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('happyhorse_router', '快乐小马智能路由', '自动合并阿里云 DashScope happyhorse 的文生/图生/参考生/编辑视频 4 个模型，自动分发请求', 0, 'system')
           ON CONFLICT (name) DO NOTHING"#,
        r#"CREATE TABLE IF NOT EXISTS happyhorse_configs (
            id SERIAL PRIMARY KEY,
            custom_model_name TEXT NOT NULL,
            custom_model_id TEXT NOT NULL,
            t2v_model TEXT NOT NULL,
            i2v_model TEXT NOT NULL,
            r2v_model TEXT NOT NULL,
            edit_model TEXT NOT NULL,
            routing_node TEXT NOT NULL UNIQUE,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_happyhorse_configs_custom_id ON happyhorse_configs (custom_model_id)",
        "COMMENT ON TABLE happyhorse_configs IS '快乐小马智能路由配置表'",
        "COMMENT ON COLUMN happyhorse_configs.custom_model_name IS '自定义模型名称'",
        "COMMENT ON COLUMN happyhorse_configs.custom_model_id IS '自定义模型ID(用户在API中请求的模型)'",
        "COMMENT ON COLUMN happyhorse_configs.t2v_model IS '绑定的文生视频模型ID'",
        "COMMENT ON COLUMN happyhorse_configs.i2v_model IS '绑定的图生视频模型ID'",
        "COMMENT ON COLUMN happyhorse_configs.r2v_model IS '绑定的参考生视频模型ID'",
        "COMMENT ON COLUMN happyhorse_configs.edit_model IS '绑定的视频编辑模型ID'",
        "COMMENT ON COLUMN happyhorse_configs.routing_node IS '生成的智能推理路由节点ID'",
        "COMMENT ON COLUMN happyhorse_configs.is_active IS '是否启用，1=启用，0=禁用'",
        r#"INSERT INTO happyhorse_configs (custom_model_name, custom_model_id, t2v_model, i2v_model, r2v_model, edit_model, routing_node, is_active)
           VALUES ('快乐小马智能路由', 'happyhorse-smart', 'happyhorse-1.0-t2v', 'happyhorse-1.0-i2v', 'happyhorse-1.0-r2v', 'happyhorse-1.0-video-edit', 'ephh-happyhorse', 1)
           ON CONFLICT (routing_node) DO NOTHING"#
    );

    // ── 初始化文件去重指纹与快乐小马微调及日志唯一标识字段（受一次性迁移保护） ──
    once_migration!(pool, "init_happyhorse_updates_v1",
        "ALTER TABLE playground_assets ADD COLUMN IF NOT EXISTS file_hash TEXT DEFAULT ''",
        "COMMENT ON COLUMN playground_assets.file_hash IS '文件内容SHA256哈希，用于幂等去重'",
        "CREATE INDEX IF NOT EXISTS idx_pg_assets_file_hash ON playground_assets(file_hash)",
        "ALTER TABLE plugin_assets ADD COLUMN IF NOT EXISTS meta_fingerprint VARCHAR(128)",
        "COMMENT ON COLUMN plugin_assets.meta_fingerprint IS 'HTTP HEAD元数据指纹(URL域名路径+Content-Length+ETag/Last-Modified的SHA-256)，用于大文件快速去重，避免下载完整文件'",
        "CREATE INDEX IF NOT EXISTS idx_plugin_assets_meta_fp ON plugin_assets (meta_fingerprint)",
        r#"CREATE TABLE IF NOT EXISTS user_level_logs (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL,
            old_level TEXT NOT NULL DEFAULT '',
            old_level_name TEXT NOT NULL DEFAULT '',
            new_level TEXT NOT NULL DEFAULT '',
            new_level_name TEXT NOT NULL DEFAULT '',
            operator TEXT NOT NULL DEFAULT '',
            operator_id TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL DEFAULT 'admin',
            remark TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        "COMMENT ON TABLE user_level_logs IS '用户等级变更历史日志'",
        "COMMENT ON COLUMN user_level_logs.source IS '变更来源: admin=管理员手动, marketing=推广负责人, system=系统自动'",
        "CREATE INDEX IF NOT EXISTS idx_user_level_logs_user_id ON user_level_logs(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_user_level_logs_created_at ON user_level_logs(created_at DESC)",
        "ALTER TABLE happyhorse_logs ALTER COLUMN id TYPE BIGINT",
        "ALTER TABLE happyhorse_logs DROP COLUMN IF EXISTS status",
        "ALTER TABLE happyhorse_logs DROP COLUMN IF EXISTS latency_ms",
        "ALTER TABLE happyhorse_logs DROP COLUMN IF EXISTS error_message",
        "ALTER TABLE happyhorse_logs DROP COLUMN IF EXISTS task_id",
        "COMMENT ON TABLE happyhorse_logs IS '快乐小马智能路由转换日志表'",
        "UPDATE happyhorse_configs SET t2v_model = m.mid FROM models m WHERE happyhorse_configs.t2v_model = m.model_id AND happyhorse_configs.t2v_model != m.mid",
        "UPDATE happyhorse_configs SET i2v_model = m.mid FROM models m WHERE happyhorse_configs.i2v_model = m.model_id AND happyhorse_configs.i2v_model != m.mid",
        "UPDATE happyhorse_configs SET r2v_model = m.mid FROM models m WHERE happyhorse_configs.r2v_model = m.model_id AND happyhorse_configs.r2v_model != m.mid",
        "UPDATE happyhorse_configs SET edit_model = m.mid FROM models m WHERE happyhorse_configs.edit_model = m.model_id AND happyhorse_configs.edit_model != m.mid",
        "COMMENT ON COLUMN happyhorse_configs.t2v_model IS '绑定的文生视频模型MID(不可变标识)'",
        "COMMENT ON COLUMN happyhorse_configs.i2v_model IS '绑定的图生视频模型MID(不可变标识)'",
        "COMMENT ON COLUMN happyhorse_configs.r2v_model IS '绑定的参考生视频模型MID(不可变标识)'",
        "COMMENT ON COLUMN happyhorse_configs.edit_model IS '绑定的视频编辑模型MID(不可变标识)'",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS log_id TEXT",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_logs_log_id ON logs (log_id)"
    );



    // 回填存量数据的 log_id（使用 前缀 + 时间戳hex + id hex 拼接，保证唯一且有序，仅执行一次避免大表全扫描卡顿）
    once_migration!(pool, "backfill_logs_log_id_v1",
        "UPDATE logs SET log_id = CASE \
            WHEN task_id IS NOT NULL AND task_id != '' \
                 AND action_type IS NOT NULL AND action_type NOT IN ('', '聊天') \
            THEN 'tsk_' || lpad(to_hex((EXTRACT(EPOCH FROM created_at::timestamp) * 1000)::bigint), 12, '0') || lpad(to_hex(id), 14, '0') \
            ELSE 'log_' || lpad(to_hex((EXTRACT(EPOCH FROM created_at::timestamp) * 1000)::bigint), 12, '0') || lpad(to_hex(id), 14, '0') \
        END \
        WHERE log_id IS NULL"
    );



    // 自动清洗历史失败日志的脏扣费数据（受一次性迁移保护）
    once_migration!(pool, "clean_dirty_logs_cost_20260609",
        "UPDATE logs SET cost = 0.0, pre_deduct_gift = 0.0 WHERE status_code < 200 OR status_code >= 400"
    );

    // 自动修复历史遗留的 users.used_quota 统计不准确问题（受一次性迁移保护）
    once_migration!(pool, "fix_used_quota_v2_20260609",
        "UPDATE users u SET \
         used_quota = COALESCE((SELECT SUM(cost) FROM logs l WHERE l.user_id = u.id), 0.0), \
         gift_used_quota = COALESCE((SELECT SUM(LEAST(cost, pre_deduct_gift)) FROM logs l WHERE l.user_id = u.id), 0.0) \
         WHERE u.used_quota > 0"
    );

    // 自动修复历史遗留的 users.balance 错误并进行真实余额校准（受一次性迁移保护）
    once_migration!(pool, "fix_users_balance_20260609",
        "UPDATE users u SET \
         balance = COALESCE((SELECT SUM(amount) FROM recharge_records r WHERE r.user_id = u.id AND r.wallet_type = 'system'), 0.0) - COALESCE((SELECT SUM(cost - pre_deduct_gift) FROM logs l WHERE l.user_id = u.id), 0.0), \
         gift_balance = GREATEST(COALESCE((SELECT SUM(amount) FROM recharge_records r WHERE r.user_id = u.id AND r.wallet_type = 'gift'), 0.0) - COALESCE((SELECT SUM(pre_deduct_gift) FROM logs l WHERE l.user_id = u.id), 0.0), 0.0) \
         WHERE EXISTS (SELECT 1 FROM logs WHERE user_id = u.id) OR EXISTS (SELECT 1 FROM recharge_records WHERE user_id = u.id)"
    );

    // 修复之前对于部分退款（cost < pre_deduct_gift）导致系统余额倒贴的漏洞并校准余额（受一次性迁移保护）
    once_migration!(pool, "fix_users_balance_v2_20260609",
        "UPDATE users u SET \
         balance = COALESCE((SELECT SUM(amount) FROM recharge_records r WHERE r.user_id = u.id AND r.wallet_type = 'system'), 0.0) - COALESCE((SELECT SUM(GREATEST(cost - pre_deduct_gift, 0.0)) FROM logs l WHERE l.user_id = u.id), 0.0), \
         gift_balance = GREATEST(COALESCE((SELECT SUM(amount) FROM recharge_records r WHERE r.user_id = u.id AND r.wallet_type = 'gift'), 0.0) - COALESCE((SELECT SUM(LEAST(cost, pre_deduct_gift)) FROM logs l WHERE l.user_id = u.id), 0.0), 0.0) \
         WHERE EXISTS (SELECT 1 FROM logs WHERE user_id = u.id) OR EXISTS (SELECT 1 FROM recharge_records WHERE user_id = u.id)"
    );

    // ── 营销、计费规则、渠道倍率与高可用令牌等扩展列定义，受一次性迁移保护 ──
    once_migration!(pool, "marketing_billing_channel_extensions_v1",
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS members_can_set_pay BIGINT NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN marketing_teams.members_can_set_pay IS '团队成员是否可以设置推广用户的支付权限(0=否,1=是)'",
        "ALTER TABLE billing_rules ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN billing_rules.sort_order IS '排序，数字越大越靠前'",
        "ALTER TABLE forward_rules ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN forward_rules.sort_order IS '排序序号，数字越大越靠前'",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS rate DOUBLE PRECISION NOT NULL DEFAULT 1.0",
        "COMMENT ON COLUMN channels.rate IS '倍率'",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS rate DOUBLE PRECISION NOT NULL DEFAULT 1.0",
        "COMMENT ON COLUMN channel_configs.rate IS '倍率'",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS high_availability INTEGER NOT NULL DEFAULT 1",
        "COMMENT ON COLUMN api_tokens.high_availability IS '是否开启高可用密钥功能(0=禁用,1=启用)'"
    );

    // ── 初始化高可用密钥渠道、指纹与令牌维度限额结构及内置配置（受一次性迁移保护） ──
    once_migration!(pool, "init_high_availability_updates_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category, allowed_levels, created_at, updated_at)
           VALUES ('high_availability_channel', '高可用上游渠道系统插件', '启用后，支持管理后台配置高可用渠道组，支持多上游自动防灾切换与按子渠道倍率计费模式。', 1, 'system_builtin', 'all', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
           ON CONFLICT (name) DO NOTHING"#,
        r#"INSERT INTO plugin_configs (plugin_name, config_key, config_value, created_at, updated_at)
           VALUES
           ('high_availability_channel', 'ha_max_retries', '3', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ('high_availability_channel', 'ha_cooldown_429', '60', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ('high_availability_channel', 'ha_cooldown_network', '300', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
           ('high_availability_channel', 'ha_cooldown_auth', '1800', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
           ON CONFLICT (plugin_name, config_key) DO NOTHING"#,
        "UPDATE users SET username = SUBSTRING(username FROM 1 FOR 48) WHERE char_length(username) > 48",
        "UPDATE users SET nickname = SUBSTRING(nickname FROM 1 FOR 24) WHERE char_length(nickname) > 24",
        "ALTER TABLE users ALTER COLUMN username TYPE VARCHAR(48)",
        "ALTER TABLE users ALTER COLUMN nickname TYPE VARCHAR(24)",
        "UPDATE api_tokens SET name = CASE WHEN SUBSTRING(REGEXP_REPLACE(name, '[^\\w ]|_', '', 'g') FROM 1 FOR 36) = '' THEN 'default' ELSE SUBSTRING(REGEXP_REPLACE(name, '[^\\w ]|_', '', 'g') FROM 1 FOR 36) END WHERE name !~ '^([^\\W_]| )+$' OR CHAR_LENGTH(name) > 36",
        "ALTER TABLE api_tokens DROP CONSTRAINT IF EXISTS chk_api_tokens_name",
        "ALTER TABLE api_tokens ADD CONSTRAINT chk_api_tokens_name CHECK (char_length(name) <= 36 AND name ~ '^([^\\W_]| )+$')",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS daily_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1.0",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS daily_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS weekly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1.0",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS weekly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS monthly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1.0",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS monthly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS last_reset_day TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS last_reset_week TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS last_reset_month TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN channel_configs.priority IS '请求优先级'",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS weight INTEGER NOT NULL DEFAULT 1",
        "COMMENT ON COLUMN channel_configs.weight IS '请求权重'",
        "UPDATE plugins SET category = 'system_builtin', is_enabled = 1 WHERE name IN ('high_availability_channel', 'site_icons')"
    );

    // ── 火山引擎画质增强与字幕擦除插件条件编译迁移 ──
    #[cfg(feature = "plugin_volcengine_enhance")]
    {
        let volc_enhance_done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sys_migration_history WHERE id = 'volcengine_enhance_init_v1'")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        if volc_enhance_done == 0 {
            tracing::info!("开始执行火山引擎画质增强与字幕擦除插件迁移与初始化...");
            // 1. 注册插件 (指定 category = 'system', 标识为系统增强插件，此处加上数据库字段意义的备注说明方便维护)
            let _ = sqlx::query(
                "INSERT INTO plugins (name, title, description, is_enabled, allowed_levels, category, created_at, updated_at) \
                 VALUES ('volcengine_enhance', '火山引擎 AI MediaKit 插件', \
                 '集成火山引擎 AI MediaKit，提供视频画质增强（标准版、专业版、极速版、大模型版）与字幕擦除（标准版、精细版）能力，支持按规格阶梯计费。', \
                 0, 'all', 'system', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
                 ON CONFLICT (name) DO UPDATE SET title = EXCLUDED.title"
            ).execute(pool).await;

            // 兼容处理：对于已经插入过的旧记录，更新插件的显示标题名称
            let _ = sqlx::query(
                "UPDATE plugins SET title = '火山引擎 AI MediaKit 插件' WHERE name = 'volcengine_enhance'"
            ).execute(pool).await;

            // 2. 批量拉取映射 ID，规避复杂嵌套子查询，确保服务商、API 提供商和模型类型都获取到
            let volc_provider_id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM model_providers WHERE name = '火山引擎' LIMIT 1"
            ).fetch_optional(pool).await.unwrap_or(None);

            let volc_api_provider_id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM model_api_providers WHERE name ILIKE '%火山%' OR name ILIKE '%volcengine%' LIMIT 1"
            ).fetch_optional(pool).await.unwrap_or(None);

            // 获取"视频增强"类型 ID（用于 6 个预置模型）
            let enhance_type_id: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM model_types WHERE name = '视频增强' LIMIT 1"
            ).fetch_optional(pool).await.unwrap_or(None);

            // 注册 4 个细分版本的视频画质计费规则 (按秒换算)
            let _ = sqlx::query(
                "INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, pricing_tiers, extended_config, is_system, provider_id, type_id) \
                 SELECT '火山 MediaKit 视频画质增强 (标准版)', 'duration', 0.0, 0.0, 0.0, 0.0125, 'video_quality', \
                 '[{\"resolution\":\"720p\",\"fps_range\":\"<=30\",\"rate\":0.0125,\"enabled\":true},{\"resolution\":\"720p\",\"fps_range\":\">30\",\"rate\":0.025,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\"<=30\",\"rate\":0.025,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\">30\",\"rate\":0.05,\"enabled\":true},{\"resolution\":\"2k\",\"fps_range\":\"<=30\",\"rate\":0.05,\"enabled\":true},{\"resolution\":\"2k\",\"fps_range\":\">30\",\"rate\":0.10,\"enabled\":true},{\"resolution\":\"4k\",\"fps_range\":\"<=30\",\"rate\":0.10,\"enabled\":true},{\"resolution\":\"4k\",\"fps_range\":\">30\",\"rate\":0.20,\"enabled\":true}]', \
                 '{}', 1, $1, $2 \
                 WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (标准版)')"
            )
            .bind(volc_provider_id)
            .bind(enhance_type_id)
            .execute(pool).await;

            let _ = sqlx::query(
                "INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, pricing_tiers, extended_config, is_system, provider_id, type_id) \
                 SELECT '火山 MediaKit 视频画质增强 (专业版)', 'duration', 0.0, 0.0, 0.0, 0.125, 'video_quality', \
                 '[{\"resolution\":\"720p\",\"fps_range\":\"<=30\",\"rate\":0.125,\"enabled\":true},{\"resolution\":\"720p\",\"fps_range\":\">30\",\"rate\":0.25,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\"<=30\",\"rate\":0.25,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\">30\",\"rate\":0.50,\"enabled\":true},{\"resolution\":\"2k\",\"fps_range\":\"<=30\",\"rate\":0.50,\"enabled\":true},{\"resolution\":\"2k\",\"fps_range\":\">30\",\"rate\":1.00,\"enabled\":true},{\"resolution\":\"4k\",\"fps_range\":\"<=30\",\"rate\":1.00,\"enabled\":true},{\"resolution\":\"4k\",\"fps_range\":\">30\",\"rate\":2.00,\"enabled\":true}]', \
                 '{}', 1, $1, $2 \
                 WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (专业版)')"
            )
            .bind(volc_provider_id)
            .bind(enhance_type_id)
            .execute(pool).await;

            let _ = sqlx::query(
                "INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, pricing_tiers, extended_config, is_system, provider_id, type_id) \
                 SELECT '火山 MediaKit 视频画质增强 (极速版)', 'duration', 0.0, 0.0, 0.0, 0.00333333, 'video_quality', \
                 '[{\"resolution\":\"720p\",\"fps_range\":\"<=30\",\"rate\":0.00333333,\"enabled\":true},{\"resolution\":\"720p\",\"fps_range\":\">30\",\"rate\":0.00666667,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\"<=30\",\"rate\":0.00666667,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\">30\",\"rate\":0.01333333,\"enabled\":true},{\"resolution\":\"2k\",\"fps_range\":\"<=30\",\"rate\":0.01333333,\"enabled\":true},{\"resolution\":\"2k\",\"fps_range\":\">30\",\"rate\":0.02666667,\"enabled\":true},{\"resolution\":\"4k\",\"fps_range\":\"<=30\",\"rate\":0.02666667,\"enabled\":true},{\"resolution\":\"4k\",\"fps_range\":\">30\",\"rate\":0.05333333,\"enabled\":true}]', \
                 '{}', 1, $1, $2 \
                 WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (极速版)')"
            )
            .bind(volc_provider_id)
            .bind(enhance_type_id)
            .execute(pool).await;

            let _ = sqlx::query(
                "INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, pricing_tiers, extended_config, is_system, provider_id, type_id) \
                 SELECT '火山 MediaKit 视频画质增强 (大模型版)', 'duration', 0.0, 0.0, 0.0, 0.04166667, 'video_quality', \
                 '[{\"resolution\":\"720p\",\"fps_range\":\"<=30\",\"rate\":0.04166667,\"enabled\":true},{\"resolution\":\"720p\",\"fps_range\":\">30\",\"rate\":0.08333333,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\"<=30\",\"rate\":0.08333333,\"enabled\":true},{\"resolution\":\"1080p\",\"fps_range\":\">30\",\"rate\":0.16666667,\"enabled\":true}]', \
                 '{}', 1, $1, $2 \
                 WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (大模型版)')"
            )
            .bind(volc_provider_id)
            .bind(enhance_type_id)
            .execute(pool).await;

            let rule_id_standard: Option<i64> = sqlx::query_scalar("SELECT id FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (标准版)'").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_professional: Option<i64> = sqlx::query_scalar("SELECT id FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (专业版)'").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_fast: Option<i64> = sqlx::query_scalar("SELECT id FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (极速版)'").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_generative: Option<i64> = sqlx::query_scalar("SELECT id FROM billing_rules WHERE name = '火山 MediaKit 视频画质增强 (大模型版)'").fetch_optional(pool).await.unwrap_or(None);

            // 注册 2 个细分版本的字幕擦除计费规则 (按秒换算)
            let _ = sqlx::query(
                "INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, pricing_tiers, extended_config, is_system, provider_id, type_id) \
                 SELECT '火山 MediaKit 视频字幕擦除 (标准版)', 'duration', 0.0, 0.0, 0.0, 0.00666667, 'standard', \
                 '[]', \
                 '{}', 1, $1, $2 \
                 WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = '火山 MediaKit 视频字幕擦除 (标准版)')"
            )
            .bind(volc_provider_id)
            .bind(enhance_type_id)
            .execute(pool).await;

            let _ = sqlx::query(
                "INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, pricing_tiers, extended_config, is_system, provider_id, type_id) \
                 SELECT '火山 MediaKit 视频字幕擦除 (精细版)', 'duration', 0.0, 0.0, 0.0, 0.01666667, 'standard', \
                 '[]', \
                 '{}', 1, $1, $2 \
                 WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = '火山 MediaKit 视频字幕擦除 (精细版)')"
            )
            .bind(volc_provider_id)
            .bind(enhance_type_id)
            .execute(pool).await;

            let rule_id_erase_standard: Option<i64> = sqlx::query_scalar("SELECT id FROM billing_rules WHERE name = '火山 MediaKit 视频字幕擦除 (标准版)'").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_erase_pro: Option<i64> = sqlx::query_scalar("SELECT id FROM billing_rules WHERE name = '火山 MediaKit 视频字幕擦除 (精细版)'").fetch_optional(pool).await.unwrap_or(None);

            // 3. 注册 4 个火山 MediaKit 内置转发规则，使用安全的 WHERE NOT EXISTS 语法防重，避开 ON CONFLICT 报错
            let preset_rules = vec![
                (
                    "火山 MediaKit 视频画质增强 (标准/专业版)",
                    "volcengine",
                    "火山画质增强标准版与专业版通用转发规则，自动进行路径和请求体参数转换，支持异步任务轮询。",
                    r#"{"target_type":"volcengine_media_enhance","path_rewrite":{"old":"/v1/video/generations","new":"/api/v1/tools/enhance-video"},"poll_path":"/api/v1/tasks/${task_id}","auth_type":"bearer"}"#
                ),
                (
                    "火山 MediaKit 视频画质增强 (极速版)",
                    "volcengine",
                    "火山画质增强极速版专用转发规则，自动转发至 enhance-video-fast，支持异步任务轮询。",
                    r#"{"target_type":"volcengine_media_enhance","path_rewrite":{"old":"/v1/video/generations","new":"/api/v1/tools/enhance-video-fast"},"poll_path":"/api/v1/tasks/${task_id}","auth_type":"bearer"}"#
                ),
                (
                    "火山 MediaKit 视频画质增强 (大模型版)",
                    "volcengine",
                    "火山画质增强大模型版专用转发规则，自动转发至 enhance-video-generative，支持异步任务轮询。",
                    r#"{"target_type":"volcengine_media_enhance","path_rewrite":{"old":"/v1/video/generations","new":"/api/v1/tools/enhance-video-generative"},"poll_path":"/api/v1/tasks/${task_id}","auth_type":"bearer"}"#
                ),
                (
                    "火山 MediaKit 视频字幕擦除",
                    "volcengine",
                    "火山视频字幕擦除（标准/精细版）通用转发规则，自动转发至 erase-video-subtitle，支持异步任务轮询。",
                    r#"{"target_type":"volcengine_media_enhance","path_rewrite":{"old":"/v1/video/generations","new":"/api/v1/tools/erase-video-subtitle"},"poll_path":"/api/v1/tasks/${task_id}","auth_type":"bearer"}"#
                )
            ];

            for (name, rtype, desc, config) in &preset_rules {
                let _ = sqlx::query(
                    "INSERT INTO forward_rules (name, rule_type, description, config_json, category, is_system, eid) \
                     SELECT $1, $2, $3, $4, '视频', 1, '1' || lpad((floor(random() * 10000)::int)::text, 4, '0') \
                     WHERE NOT EXISTS (SELECT 1 FROM forward_rules WHERE name = $1)"
                )
                .bind(name).bind(rtype).bind(desc).bind(config)
                .execute(pool).await;
            }

            // 4. 获取刚注册好的内置规则 ID 映射
            let rule_id_sd_pf: Option<i64> = sqlx::query_scalar("SELECT id FROM forward_rules WHERE name = '火山 MediaKit 视频画质增强 (标准/专业版)'").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_ft: Option<i64> = sqlx::query_scalar("SELECT id FROM forward_rules WHERE name = '火山 MediaKit 视频画质增强 (极速版)'").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_gt: Option<i64> = sqlx::query_scalar("SELECT id FROM forward_rules WHERE name = '火山 MediaKit 视频画质增强 (大模型版)'").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_erase: Option<i64> = sqlx::query_scalar("SELECT id FROM forward_rules WHERE name = '火山 MediaKit 视频字幕擦除'").fetch_optional(pool).await.unwrap_or(None);

            // 5. 初始化 6 个画质增强预置模型，显式绑定 provider_id (火山引擎) 、默认转发规则 forward_rule_ids 以及默认计费规则 billing_rule_id
            let preset_models = vec![
                ("vve-sd", "火山画质增强-标准版", "volc_video_enhance_standard", rule_id_sd_pf, rule_id_standard),
                ("vve-pf", "火山画质增强-专业版", "volc_video_enhance_professional", rule_id_sd_pf, rule_id_professional),
                ("vve-ft", "火山画质增强-极速版", "volc_video_enhance_fast", rule_id_ft, rule_id_fast),
                ("vve-gt", "火山画质增强-大模型版", "volc_video_enhance_generative", rule_id_gt, rule_id_generative),
                ("vvs-er", "火山字幕擦除-标准版", "volc_video_subtitle_erase", rule_id_erase, rule_id_erase_standard),
                ("vvs-ep", "火山字幕擦除-精细版", "volc_video_subtitle_erase_pro", rule_id_erase, rule_id_erase_pro),
            ];

            for (mid, name, model_id, rule_id, billing_rule_id) in &preset_models {
                let rule_ids_json = rule_id.map(|id| format!("[{}]", id));
                let _ = sqlx::query(
                    "INSERT INTO models (mid, name, model_id, provider_id, api_provider_id, type_id, forward_rule_ids, billing_rule_id, is_active, \
                     remark, created_at, updated_at) \
                     SELECT $1, $2, $3, $4, $5, $6, $7, $8, 0, '火山引擎画质增强/字幕擦除插件预置模型，请勿删除', \
                     CURRENT_TIMESTAMP, CURRENT_TIMESTAMP \
                     WHERE NOT EXISTS (SELECT 1 FROM models WHERE mid = $1)"
                )
                .bind(mid).bind(name).bind(model_id)
                .bind(volc_provider_id)
                .bind(volc_api_provider_id)
                .bind(enhance_type_id)
                .bind(rule_ids_json)
                .bind(billing_rule_id)
                .execute(pool).await;
            }

            // 5.5 初始化两个豆包级联画质增强模型种子数据，强制绑定到级联计费规则与转发规则
            let video_type_id: Option<i64> = sqlx::query_scalar("SELECT id FROM model_types WHERE name = '视频' LIMIT 1").fetch_optional(pool).await.unwrap_or(None);
            let rule_id_cascade_billing: Option<i64> = sqlx::query_scalar("SELECT id FROM billing_rules WHERE name = '火山级联画质增强默认计费' LIMIT 1").fetch_optional(pool).await.unwrap_or(None);

            let cascade_models = vec![
                ("dbs-sr", "豆包 Seedance 2.0 (画质增强级联)", "Doubao-seedance-2-0-sr", "doubao-seedance-2-0-260128", 30.0),
                ("dbs-fs", "豆包 Seedance 2.0 极速版 (画质增强级联)", "Doubao-seedance-2-0-fast-sr", "doubao-seedance-2-0-fast-260128", 30.0),
            ];

            for (mid, name, model_id, alias, pre_deduct) in &cascade_models {
                let _ = sqlx::query(
                    "INSERT INTO models (mid, name, model_id, model_id_alias, provider_id, api_provider_id, type_id, group_ratios, billing_rule_id, pre_deduction, is_active, remark, created_at, updated_at) \
                     SELECT $1, $2, $3, $4, $5, $6, $7, '{\"default\":1.0}', $8, $9, 0, '火山方舟级联画质增强模型，请勿删除', \
                     CURRENT_TIMESTAMP, CURRENT_TIMESTAMP \
                     WHERE NOT EXISTS (SELECT 1 FROM models WHERE mid = $1)"
                )
                .bind(mid).bind(name).bind(model_id).bind(alias)
                .bind(volc_provider_id)
                .bind(volc_api_provider_id)
                .bind(video_type_id)
                .bind(rule_id_cascade_billing)
                .bind(pre_deduct)
                .execute(pool).await;
            }

            let _ = sqlx::query("INSERT INTO sys_migration_history (id) VALUES ('volcengine_enhance_init_v1')").execute(pool).await;
            tracing::info!("火山引擎画质增强插件初始化完成");
        }
    }


    // 7. PostgreSQL 18.4 专用性能与稳定性优化：引入覆盖索引（Covering Indexes）加速大表查询与统计（受一次性迁移保护）
    once_migration!(pool, "pg18_performance_optimizations_v1",
        "CREATE INDEX IF NOT EXISTS idx_logs_user_dashboard_covering ON logs (user_id, created_at DESC) INCLUDE (cost, prompt_tokens, completion_tokens, cached_tokens)",
        "CREATE INDEX IF NOT EXISTS idx_logs_admin_dashboard_covering ON logs (created_at DESC) INCLUDE (cost, prompt_tokens, completion_tokens, cached_tokens)"
    );

    // 8. 将 'playground' 插件的 title 从 '模型体验中心' 修改为 '模型创作中心'（受一次性迁移保护）
    once_migration!(pool, "rename_playground_title_to_creation_center_20260621",
        "UPDATE plugins SET title = '模型创作中心' WHERE name = 'playground'"
    );

    // 9. 将系统默认菜单配置中 '/playground' 的 label_zh 从 '体验中心' 或 '操场' 修改为 '创作中心'（受一次性迁移保护）
    once_migration!(pool, "update_menu_playground_label_to_creation_center_20260621",
        "UPDATE settings SET value = replace(\
            replace(\
                replace(\
                    replace(value, '\"label_zh\":\"体验中心\"', '\"label_zh\":\"创作中心\"'), \
                    '\"label_zh\": \"体验中心\"', '\"label_zh\": \"创作中心\"'\
                ), \
                '\"label_zh\":\"操场\"', '\"label_zh\":\"创作中心\"'\
            ), \
            '\"label_zh\": \"操场\"', '\"label_zh\": \"创作中心\"'\
        ) WHERE key = 'menu_config_settings'"
    );

    // ── DocsApi 站点 API 教程文档增强插件初始化 ──
    let docs_api_init_done: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sys_migration_history WHERE id = 'docs_api_init_v5'")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if docs_api_init_done == 0 {
        // 1. 创建 plugin_docs 表
        let _ = sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS plugin_docs (
                id SERIAL PRIMARY KEY,
                parent_id INTEGER NULL REFERENCES plugin_docs(id) ON DELETE CASCADE,
                title VARCHAR(255) NOT NULL,
                content TEXT DEFAULT '',
                is_dir INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0,
                is_active INTEGER NOT NULL DEFAULT 1,
                slug VARCHAR(255) DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (now()::text),
                updated_at TEXT NOT NULL DEFAULT (now()::text)
            )"#
        ).execute(pool).await;

        // 2. 提前创建 plugin_docs_intl 国际化表，保证种子数据 seed_default_docs_direct 可以顺利写入翻译数据
        let _ = sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS plugin_docs_intl (
                id SERIAL PRIMARY KEY,
                doc_id INTEGER NOT NULL REFERENCES plugin_docs(id) ON DELETE CASCADE,
                lang VARCHAR(10) NOT NULL,
                title VARCHAR(255) NOT NULL,
                content TEXT DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (now()::text),
                updated_at TEXT NOT NULL DEFAULT (now()::text),
                UNIQUE(doc_id, lang)
            )"#
        ).execute(pool).await;

        // 3. 注册 docs_api 插件（默认关闭）
        let _ = sqlx::query(
            "INSERT INTO plugins (name, title, description, is_enabled, allowed_levels, category, created_at, updated_at) \
             VALUES ('docs_api', 'DocsApi文档', '提供站点 API 教程的文档管理系统，支持多级目录大纲与 Markdown 内容手动编辑。', 0, 'all', 'user', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP) \
             ON CONFLICT (name) DO UPDATE SET title = EXCLUDED.title, description = EXCLUDED.description, is_enabled = EXCLUDED.is_enabled"
        ).execute(pool).await;

        // 4. 写入初始数据
        if let Err(e) = crate::api::plugins::docs_api::seed_default_docs_direct(pool).await {
            tracing::error!("Failed to seed default docs: {:?}", e);
        }

        let _ = sqlx::query("INSERT INTO sys_migration_history (id) VALUES ('docs_api_init_v5')").execute(pool).await;
        tracing::info!("✅ DocsApi 文档插件初始化完成");
    }

    // ── DocsApi 插件新增 slug 字段（受一次性迁移保护） ──
    once_migration!(pool, "docs_api_add_slug_v1",
        "ALTER TABLE plugin_docs ADD COLUMN IF NOT EXISTS slug VARCHAR(255) DEFAULT ''"
    );

    // ── 级联转发规则补充默认 res_mul（分辨率倍率，缺省 1.0 不影响现网计价）──
    once_migration!(pool, "cascade_res_mul_v1",
        r#"UPDATE forward_rules
           SET config_json = (COALESCE(config_json::jsonb, '{}'::jsonb) || '{"res_mul":{"720p":2.15,"1080p":2.25,"2k":2.5,"4k":4.0}}'::jsonb)::text
           WHERE name = '火山方舟 级联视频生成'
             AND (config_json::jsonb -> 'res_mul') IS NULL"#
    );

    // ── playground_projects 新增 is_pinned 字段（受一次性迁移保护） ──
    once_migration!(pool, "pg_projects_add_is_pinned_v1",
        "ALTER TABLE playground_projects ADD COLUMN IF NOT EXISTS is_pinned INTEGER NOT NULL DEFAULT 0"
    );

    // ── logs 表新增 is_completed 字段：标识任务是否已终结 ──
    // ── 初始化日志终结标记及条件索引（受一次性迁移保护） ──
    once_migration!(pool, "logs_add_is_completed_v1",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS is_completed SMALLINT NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN logs.is_completed IS '任务是否已终结(1=已完成,0=进行中或待结算)'",
        "UPDATE logs SET is_completed = 1 WHERE is_completed = 0 AND (billing_detail IS NULL OR billing_detail NOT LIKE '%冻结%')",
        "CREATE INDEX IF NOT EXISTS idx_logs_is_completed_pending ON logs (id DESC) WHERE is_completed = 0",
        "UPDATE logs SET is_completed = 1 WHERE is_completed = 0 AND (billing_detail LIKE '[测试渠道，不扣费]%' OR endpoint LIKE 'test|%')"
    );

    // ── 统一合并的零散 DML 一次性回填 ──
    once_migration!(pool, "backfill_misc_data_v1",
        "UPDATE user_levels SET is_default = 1 WHERE group_key = 'default' AND NOT EXISTS (SELECT 1 FROM user_levels WHERE is_default = 1)",
        "UPDATE forward_rules SET category = '音频' WHERE category = '语音'",
        "UPDATE forward_rules SET rule_type = 'aliyun' WHERE name LIKE '%阿里百炼%' AND rule_type != 'aliyun'",
        "UPDATE forward_rules SET config_json = '{\"target_type\":\"anthropic\",\"path_rewrite\":{\"old\":\"/v1/chat/completions\",\"new\":\"/v1/messages\"},\"auth_type\":\"x-api-key\"}', description = '将 OpenAI 格式请求转换为 Anthropic Messages API 格式，接口 /v1/messages' WHERE name = 'Anthropic 原生转化' AND is_system = 1",
        "UPDATE forward_rules SET eid = '1' || floor(random() * 9000 + 1000)::text WHERE eid = '' OR eid IS NULL",
        "UPDATE billing_rules SET pid = '7' || floor(random() * 9000 + 1000)::text WHERE is_system = 1 AND (pid = '' OR pid IS NULL)",
        "UPDATE billing_rules SET pid = '6' || floor(random() * 9000 + 1000)::text WHERE is_system = 0 AND (pid = '' OR pid IS NULL)",
        "UPDATE channel_configs SET yid = '3' || floor(random() * 9000 + 1000)::text WHERE yid = '' OR yid IS NULL",
        "UPDATE model_types SET logo = 'sora' WHERE name = '视频' AND (logo IS NULL OR logo = '')",
        "UPDATE model_types SET logo = 'midjourney' WHERE name = '图片' AND (logo IS NULL OR logo = '')",
        "UPDATE model_types SET logo = 'suno' WHERE name = '音频' AND (logo IS NULL OR logo = '')",
        "UPDATE model_types SET logo = 'chatgpt' WHERE name = '聊天' AND (logo IS NULL OR logo = '')",
        // 仅回填空 logo/remark，禁止改写 sort_order（管理端自定义排序升级后须保留）
        "UPDATE model_types SET logo = CASE WHEN logo IS NULL OR logo = '' THEN 'volcengine' ELSE logo END, remark = CASE WHEN remark IS NULL OR remark = '' THEN '视频画质增强与字幕擦除处理模型' ELSE remark END WHERE name = '视频增强' AND (logo IS NULL OR logo = '' OR remark IS NULL OR remark = '')"
    );

    // ── usage_daily_stats 每日使用统计落地表及 logs 高性能查询索引（受一次性迁移保护） ──
    once_migration!(pool, "add_usage_daily_stats_v1",
        r#"CREATE TABLE IF NOT EXISTS usage_daily_stats (
            id BIGSERIAL PRIMARY KEY,
            stat_date DATE NOT NULL,
            user_id TEXT NOT NULL,
            model TEXT NOT NULL,
            token_id BIGINT NOT NULL DEFAULT -1,
            channel_id BIGINT NOT NULL DEFAULT -1,
            action_type TEXT NOT NULL DEFAULT '',
            total_requests BIGINT NOT NULL DEFAULT 0,
            total_tokens BIGINT NOT NULL DEFAULT 0,
            total_cost DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            total_pre_deduct_gift DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            success_count BIGINT NOT NULL DEFAULT 0,
            fail_count BIGINT NOT NULL DEFAULT 0,
            ext_json JSONB,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )"#,
        "COMMENT ON TABLE usage_daily_stats IS '使用量每日统计表 (Lambda 离线统计落地表)'",
        "COMMENT ON COLUMN usage_daily_stats.id IS '自增主键'",
        "COMMENT ON COLUMN usage_daily_stats.stat_date IS '统计日期 (YYYY-MM-DD)'",
        "COMMENT ON COLUMN usage_daily_stats.user_id IS '用户ID'",
        "COMMENT ON COLUMN usage_daily_stats.model IS '模型名称'",
        "COMMENT ON COLUMN usage_daily_stats.token_id IS '令牌ID (-1代表无令牌)'",
        "COMMENT ON COLUMN usage_daily_stats.channel_id IS '渠道ID (-1代表无渠道)'",
        "COMMENT ON COLUMN usage_daily_stats.action_type IS '动作类型(聊天,图片,视频等)'",
        "COMMENT ON COLUMN usage_daily_stats.total_requests IS '总请求数'",
        "COMMENT ON COLUMN usage_daily_stats.total_tokens IS '总消费 tokens 数量'",
        "COMMENT ON COLUMN usage_daily_stats.total_cost IS '总消费金额'",
        "COMMENT ON COLUMN usage_daily_stats.total_pre_deduct_gift IS '总消费赠送余额金额'",
        "COMMENT ON COLUMN usage_daily_stats.success_count IS '状态码 2xx 的成功请求数'",
        "COMMENT ON COLUMN usage_daily_stats.fail_count IS '状态码非 2xx 的失败请求数'",
        "COMMENT ON COLUMN usage_daily_stats.ext_json IS '扩展元数据 JSONB (供未来新指标无感扩展使用)'",
        "CREATE UNIQUE INDEX IF NOT EXISTS uidx_usage_daily_stats_dims ON usage_daily_stats (stat_date, user_id, model, token_id, channel_id, action_type)",
        "CREATE INDEX IF NOT EXISTS idx_usage_daily_stats_date_user ON usage_daily_stats (stat_date, user_id)",
        "CREATE INDEX IF NOT EXISTS idx_logs_created_at_timestamptz ON logs (created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_logs_user_created_timestamptz ON logs (user_id, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_logs_date_created_at ON logs ((SUBSTRING(created_at FROM 1 FOR 10)))",
        "CREATE INDEX IF NOT EXISTS idx_logs_stats_opt ON logs (user_id, created_at DESC) INCLUDE (cost, status_code, pre_deduct_gift)",
        "CREATE INDEX IF NOT EXISTS idx_logs_created_at_stats_opt ON logs (created_at DESC) INCLUDE (cost, status_code, pre_deduct_gift)"
    );

    once_migration!(pool, "add_ha_cooldown_404_v2",
        r#"INSERT INTO plugin_configs (plugin_name, config_key, config_value, created_at, updated_at)
           VALUES ('high_availability_channel', 'ha_cooldown_404', '3', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
           ON CONFLICT (plugin_name, config_key) 
           DO UPDATE SET config_value = '3', updated_at = CURRENT_TIMESTAMP 
           WHERE plugin_configs.config_value = '10'"#
    );

    // ── 火山方舟视频监控插件：主账号、Endpoint绑定、视频任务、分账账单 ──
    once_migration!(pool, "add_volc_ark_monitor_v1",
        // 主账号凭证表（支持多火山账号）
        r#"CREATE TABLE IF NOT EXISTS ark_accounts (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            volc_account_id TEXT NOT NULL DEFAULT '',
            access_key TEXT NOT NULL,
            secret_key TEXT NOT NULL,
            region TEXT NOT NULL DEFAULT 'cn-beijing',
            remark TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
        )"#,
        "COMMENT ON TABLE ark_accounts IS '火山方舟主账号凭证表（AK/SK）'",
        "COMMENT ON COLUMN ark_accounts.id IS '自增主键'",
        "COMMENT ON COLUMN ark_accounts.name IS '账号别名，全局唯一'",
        "COMMENT ON COLUMN ark_accounts.volc_account_id IS '火山官方账号ID (AccountId)'",
        "COMMENT ON COLUMN ark_accounts.access_key IS '火山引擎 AccessKey'",
        "COMMENT ON COLUMN ark_accounts.secret_key IS '火山引擎 SecretKey'",
        "COMMENT ON COLUMN ark_accounts.region IS 'API调用区域，默认cn-beijing'",
        "COMMENT ON COLUMN ark_accounts.remark IS '管理员备注'",
        // Endpoint与内部用户的绑定关系表
        r#"CREATE TABLE IF NOT EXISTS ark_endpoint_bindings (
            id SERIAL PRIMARY KEY,
            account_id INTEGER NOT NULL REFERENCES ark_accounts(id) ON DELETE CASCADE,
            endpoint_id TEXT NOT NULL,
            user_uid TEXT NOT NULL,
            api_key TEXT NOT NULL DEFAULT '',
            limit_quota DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            used_quota DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            status INTEGER NOT NULL DEFAULT 1,
            remark TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(account_id, endpoint_id)
        )"#,
        "COMMENT ON TABLE ark_endpoint_bindings IS '火山方舟Endpoint与内部用户的绑定关系'",
        "COMMENT ON COLUMN ark_endpoint_bindings.id IS '自增主键'",
        "COMMENT ON COLUMN ark_endpoint_bindings.account_id IS '关联的主账号ID'",
        "COMMENT ON COLUMN ark_endpoint_bindings.endpoint_id IS '火山方舟接入点ID (ep-xxxx)'",
        "COMMENT ON COLUMN ark_endpoint_bindings.user_uid IS '关联的内部用户UID'",
        "COMMENT ON COLUMN ark_endpoint_bindings.api_key IS '绑定的火山方舟静态API Key'",
        "COMMENT ON COLUMN ark_endpoint_bindings.limit_quota IS '消费额度上限(元)，0=不限制'",
        "COMMENT ON COLUMN ark_endpoint_bindings.used_quota IS '已消费金额(元)，由分账账单同步更新'",
        "COMMENT ON COLUMN ark_endpoint_bindings.status IS '状态: 1=正常 0=已熔断停用'",
        "COMMENT ON COLUMN ark_endpoint_bindings.remark IS '管理员备注'",
        "CREATE INDEX IF NOT EXISTS idx_ark_bindings_user ON ark_endpoint_bindings(user_uid)",
        "CREATE INDEX IF NOT EXISTS idx_ark_bindings_endpoint ON ark_endpoint_bindings(endpoint_id)",
        // 视频任务缓存表（拉取自ListVideos）
        r#"CREATE TABLE IF NOT EXISTS ark_video_tasks (
            id BIGSERIAL PRIMARY KEY,
            account_id INTEGER NOT NULL,
            endpoint_id TEXT NOT NULL DEFAULT '',
            task_id TEXT NOT NULL UNIQUE,
            model TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT '',
            duration DOUBLE PRECISION,
            resolution TEXT NOT NULL DEFAULT '',
            created_time TEXT NOT NULL DEFAULT '',
            split_amount DOUBLE PRECISION NOT NULL DEFAULT 0.0,
            is_estimated BOOLEAN NOT NULL DEFAULT TRUE,
            total_tokens BIGINT NOT NULL DEFAULT 0,
            synced_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
            raw_response JSONB NOT NULL DEFAULT '{}'
        )"#,
        "COMMENT ON TABLE ark_video_tasks IS '火山方舟视频任务缓存(来自ListVideos API)'",
        "COMMENT ON COLUMN ark_video_tasks.id IS '自增主键'",
        "COMMENT ON COLUMN ark_video_tasks.account_id IS '所属主账号ID'",
        "COMMENT ON COLUMN ark_video_tasks.endpoint_id IS '归属接入点ID'",
        "COMMENT ON COLUMN ark_video_tasks.task_id IS '火山视频任务唯一ID'",
        "COMMENT ON COLUMN ark_video_tasks.model IS '使用的底座模型名称'",
        "COMMENT ON COLUMN ark_video_tasks.status IS '任务状态(succeed/failed/running等)'",
        "COMMENT ON COLUMN ark_video_tasks.duration IS '视频时长(秒)'",
        "COMMENT ON COLUMN ark_video_tasks.resolution IS '视频分辨率'",
        "COMMENT ON COLUMN ark_video_tasks.created_time IS '火山侧创建时间'",
        "COMMENT ON COLUMN ark_video_tasks.split_amount IS '对应的分账账单消费金额(元)'",
        "COMMENT ON COLUMN ark_video_tasks.is_estimated IS '消费金额是否为估算值(true=估算, false=账单确认)'",
        "COMMENT ON COLUMN ark_video_tasks.total_tokens IS '视频生成消耗的总 token 数'",
        "COMMENT ON COLUMN ark_video_tasks.raw_response IS '火山方舟视频返回的所有原始响应JSON(大字段)'",
        "CREATE INDEX IF NOT EXISTS idx_ark_video_tasks_endpoint ON ark_video_tasks(endpoint_id)",
        "CREATE INDEX IF NOT EXISTS idx_ark_video_tasks_account ON ark_video_tasks(account_id)",
        // 废弃并清理原账单表
        "DROP TABLE IF EXISTS ark_split_bills CASCADE",
        // 注册插件记录
        r#"INSERT INTO plugins (name, title, description, is_enabled, category, created_at, updated_at)
           VALUES ('volcengine_ark_monitor', '火山方舟视频监控', '基于火山方舟接入点(Endpoint)的视频任务与分账账单精密监控及超额熔断控制', 0, 'user', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
           ON CONFLICT (name) DO UPDATE SET title = EXCLUDED.title, description = EXCLUDED.description, category = EXCLUDED.category"#
    );

    // 新增用户信用额度限制和支付启用字段，修复最新代码与老版本数据库表结构不一致的问题
    once_migration!(pool, "add_user_credit_limit_and_pay_fields_v1",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS credit_limit DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS pay_enabled INTEGER NOT NULL DEFAULT 1"
    );

    once_migration!(pool, "add_channel_config_id_to_logs_v1",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS channel_config_id INTEGER"
    );

    once_migration!(pool, "add_yid_to_logs_v1",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS yid TEXT DEFAULT ''",
        "COMMENT ON COLUMN logs.yid IS '上游渠道对应的内部标识(由服务商或底层平台侧生成)'"
    );

    // 子配快照统一用 channel_config_id；展示 YID 由 JOIN channel_configs 得到
    once_migration!(pool, "drop_logs_yid_v1",
        "ALTER TABLE logs DROP COLUMN IF EXISTS yid"
    );

    once_migration!(pool, "add_ha_meltdown_whitelist_v1",
        r#"INSERT INTO plugin_configs (plugin_name, config_key, config_value, created_at, updated_at)
           VALUES ('high_availability_channel', 'ha_meltdown_whitelist', '[]', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
           ON CONFLICT (plugin_name, config_key) DO NOTHING"#
    );

    once_migration!(pool, "marketing_teams_view_logs_v1",
        "ALTER TABLE marketing_teams ADD COLUMN IF NOT EXISTS members_can_view_logs BIGINT NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN marketing_teams.members_can_view_logs IS '团队成员是否可以查询关联用户的日志记录(0=否,1=是)'"
    );

    // ── 火山方舟视频任务表新增消费金额是否为估算值字段 ──
    once_migration!(pool, "add_ark_video_tasks_is_estimated_v1",
        "ALTER TABLE ark_video_tasks ADD COLUMN IF NOT EXISTS is_estimated BOOLEAN NOT NULL DEFAULT TRUE",
        "COMMENT ON COLUMN ark_video_tasks.is_estimated IS '消费金额是否为估算值(true=估算, false=账单确认)'"
    );

    // ── 为历史遗留的缺失注释的数据库字段补齐备注 ──
    once_migration!(pool, "comment_missing_db_fields_v1",
        "COMMENT ON COLUMN users.credit_limit IS '用户信用额度限制(元)'",
        "COMMENT ON COLUMN users.pay_enabled IS '是否启用支付扣费与额度限制(0=禁用, 1=启用)'",
        "COMMENT ON COLUMN logs.channel_config_id IS '关联渠道配置表的ID'"
    );

    // ── 火山方舟视频监控插件增加调试日志启用默认配置 ──
    once_migration!(pool, "add_volc_ark_monitor_debug_log_config_v1",
        r#"INSERT INTO plugin_configs (plugin_name, config_key, config_value, created_at, updated_at)
           VALUES ('volcengine_ark_monitor', 'enable_debug_log', 'false', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
           ON CONFLICT (plugin_name, config_key) DO NOTHING"#
    );

    // ── 删除已废弃的智能路由插件 ──
    once_migration!(pool, "remove_router_flow_plugin_v1",
        "DELETE FROM plugins WHERE name = 'router_flow'"
    );

    // ── 删除已废弃的火山卡池和GPT卡池插件 ──
    once_migration!(pool, "remove_pools_plugins_v3",
        "DELETE FROM plugins WHERE name IN ('volcengine_pool', 'gptimage_pool')"
    );

    // ── 清理已移除插件残留表/字段（代码侧已无引用；须在全量节点升级到无卡池版本后执行）──
    // 覆盖：router_flow / volcengine_pool / gptimage_pool 的表、channels 孤儿列、plugin 配置与用户菜单死链
    once_migration!(pool, "drop_removed_plugin_schema_v1",
        // 子表/日志先于主表
        "DROP TABLE IF EXISTS volcengine_pool_logs CASCADE",
        "DROP TABLE IF EXISTS volcengine_pool_account_mapping CASCADE",
        "DROP TABLE IF EXISTS volcengine_pool_accounts CASCADE",
        "DROP TABLE IF EXISTS volcengine_pools CASCADE",
        "DROP TABLE IF EXISTS gptimage_pool_logs CASCADE",
        "DROP TABLE IF EXISTS gptimage_pool_account_mapping CASCADE",
        "DROP TABLE IF EXISTS gptimage_pool_accounts CASCADE",
        "DROP TABLE IF EXISTS gptimage_pools CASCADE",
        "DROP TABLE IF EXISTS router_flow_groups CASCADE",
        // channels 孤儿外联列（Channel 模型与 API 已不再读写）
        "ALTER TABLE channels DROP COLUMN IF EXISTS pool_id",
        "ALTER TABLE channels DROP COLUMN IF EXISTS gptimage_pool_id",
        // 插件元数据与配置残留（幂等）
        "DELETE FROM plugin_configs WHERE plugin_name IN ('router_flow', 'volcengine_pool', 'gptimage_pool')",
        "DELETE FROM plugins WHERE name IN ('router_flow', 'volcengine_pool', 'gptimage_pool')",
        // 用户菜单默认项中的已删页面 /smart-router（value 为 JSON 文本）
        r#"UPDATE settings SET value = (
              SELECT COALESCE(
                jsonb_set(
                  value::jsonb,
                  '{items}',
                  COALESCE((
                    SELECT jsonb_agg(elem)
                    FROM jsonb_array_elements(COALESCE(value::jsonb->'items', '[]'::jsonb)) elem
                    WHERE elem->>'key' IS DISTINCT FROM '/smart-router'
                  ), '[]'::jsonb)
                )::text,
                value
              )
            )
            WHERE key = 'menu_config_settings'
              AND value IS NOT NULL
              AND value <> ''
              AND value::jsonb->'items' @> '[{"key":"/smart-router"}]'::jsonb"#
    );

    // 令牌名称：允许字母/数字/空格/下划线/连字符（对齐前后端校验）
    once_migration!(pool, "fix_api_tokens_name_allow_underscore_v1",
        "ALTER TABLE api_tokens DROP CONSTRAINT IF EXISTS chk_api_tokens_name",
        "ALTER TABLE api_tokens DROP CONSTRAINT IF EXISTS api_tokens_name_check",
        "ALTER TABLE api_tokens ADD CONSTRAINT chk_api_tokens_name CHECK (char_length(name) <= 36 AND name ~ '^[[:alnum:]_[:space:]-]+$')"
    );

    // ── 增加用户通知订阅偏好设置字段 ──
    once_migration!(pool, "add_user_notification_preferences_v1",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS notification_preferences TEXT",
        "COMMENT ON COLUMN users.notification_preferences IS '用户的通知订阅偏好(JSON格式)'"
    );

    // ── 渠道分组分类：可自定义分类，默认图片/视频/聊天 ──
    once_migration!(pool, "init_channel_categories_v1",
        r#"CREATE TABLE IF NOT EXISTS channel_categories (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            name_en TEXT NOT NULL DEFAULT '',
            sort_order INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            is_system INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS category_id BIGINT REFERENCES channel_categories(id)",
        "INSERT INTO channel_categories (name, name_en, sort_order, is_active, is_system) VALUES ('图片', 'Image', 30, 1, 1) ON CONFLICT (name) DO UPDATE SET is_system = 1",
        "INSERT INTO channel_categories (name, name_en, sort_order, is_active, is_system) VALUES ('视频', 'Video', 20, 1, 1) ON CONFLICT (name) DO UPDATE SET is_system = 1",
        "INSERT INTO channel_categories (name, name_en, sort_order, is_active, is_system) VALUES ('聊天', 'Chat', 10, 1, 1) ON CONFLICT (name) DO UPDATE SET is_system = 1"
    );

    // 若先前误用 TIMESTAMPTZ，统一改为 TEXT 以匹配 sqlx String 映射
    once_migration!(pool, "fix_channel_categories_timestamps_v1",
        r#"DO $$
        BEGIN
          IF EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'channel_categories' AND column_name = 'created_at'
              AND data_type = 'timestamp with time zone'
          ) THEN
            ALTER TABLE channel_categories
              ALTER COLUMN created_at TYPE TEXT USING created_at::text,
              ALTER COLUMN updated_at TYPE TEXT USING updated_at::text;
          END IF;
        END $$"#
    );

    // ── 兑换码：有效期 / 总次数 / 每用户次数 + 兑换记录表 ──
    once_migration!(pool, "redemptions_limits_expiry_v1",
        "ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS expires_at TEXT",
        "ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS max_uses INTEGER NOT NULL DEFAULT 1",
        "ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS used_count INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS per_user_limit INTEGER NOT NULL DEFAULT 1",
        "UPDATE redemptions SET used_count = 1 WHERE is_used = 1 AND used_count = 0",
        r#"CREATE TABLE IF NOT EXISTS redemption_logs (
            id BIGSERIAL PRIMARY KEY,
            redemption_id BIGINT NOT NULL REFERENCES redemptions(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL,
            amount DOUBLE PRECISION NOT NULL,
            created_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_redemption_logs_code_user ON redemption_logs (redemption_id, user_id)"
    );

    once_migration!(pool, "redemptions_status_v1",
        "ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS status INTEGER NOT NULL DEFAULT 1"
    );

    // ── 渠道分组 + 上游预设：日/月/总额度 ──
    once_migration!(pool, "channel_period_quota_v1",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS daily_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS daily_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS monthly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS monthly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS last_reset_day TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS last_reset_month TEXT NOT NULL DEFAULT ''",
        "COMMENT ON COLUMN channels.daily_quota_limit IS '日额度上限(-1=无限)'",
        "COMMENT ON COLUMN channels.monthly_quota_limit IS '月额度上限(-1=无限)'",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS daily_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS daily_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS monthly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS monthly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS last_reset_day TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS last_reset_month TEXT NOT NULL DEFAULT ''",
        "COMMENT ON COLUMN channel_configs.quota_limit IS '总额度上限(-1=无限)'",
        "COMMENT ON COLUMN channel_configs.daily_quota_limit IS '日额度上限(-1=无限)'",
        "COMMENT ON COLUMN channel_configs.monthly_quota_limit IS '月额度上限(-1=无限)'"
    );

    // ── 渠道分组 + 上游预设：周额度（对齐令牌日/周/月）──
    once_migration!(pool, "channel_weekly_quota_v1",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS weekly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS weekly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channels ADD COLUMN IF NOT EXISTS last_reset_week TEXT NOT NULL DEFAULT ''",
        "COMMENT ON COLUMN channels.weekly_quota_limit IS '周额度上限(-1=无限)'",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS weekly_quota_limit DOUBLE PRECISION NOT NULL DEFAULT -1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS weekly_quota_used DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS last_reset_week TEXT NOT NULL DEFAULT ''",
        "COMMENT ON COLUMN channel_configs.weekly_quota_limit IS '周额度上限(-1=无限)'"
    );

    // ── 全库时间列 TEXT → TIMESTAMPTZ（timesystem UTC，提升 logs 等范围查询可走索引）──
    // 部署注意：logs 大表 ALTER TYPE 会持有 ACCESS EXCLUSIVE 并重写表，请安排维护窗口。
    // 周期键 last_reset_* / last_daily_reset 等仍为 TEXT（日历键，非时间戳）。
    once_migration!(pool, "timestamptz_unify_v1",
        r#"CREATE OR REPLACE FUNCTION _tb_text_to_tstz(t TEXT) RETURNS TIMESTAMPTZ AS $fn$
        BEGIN
          IF t IS NULL OR btrim(t) = '' THEN
            RETURN NULL;
          END IF;
          BEGIN
            IF substring(t from 11) LIKE '%+%'
               OR substring(t from 11) LIKE '%-%'
               OR substring(t from 11) LIKE '%Z%'
               OR position('T' in t) > 0 THEN
              RETURN t::timestamptz;
            END IF;
            RETURN (t || '+00:00')::timestamptz;
          EXCEPTION WHEN OTHERS THEN
            BEGIN
              RETURN (t || '+00:00')::timestamptz;
            EXCEPTION WHEN OTHERS THEN
              RETURN NULL;
            END;
          END;
        END;
        $fn$ LANGUAGE plpgsql IMMUTABLE"#,
        r#"DO $mig$
        DECLARE
          r RECORD;
          ddl TEXT;
          tbl_exists BOOLEAN;
          is_text BOOLEAN;
        BEGIN
          FOR r IN
            SELECT * FROM (VALUES
              ('logs', 'created_at', true),
              ('users', 'created_at', true),
              ('users', 'updated_at', true),
              ('api_tokens', 'created_at', true),
              ('api_tokens', 'updated_at', true),
              ('api_tokens', 'expires_at', false),
              ('api_tokens', 'last_used_at', false),
              ('channels', 'created_at', true),
              ('channels', 'updated_at', true),
              ('channel_configs', 'created_at', true),
              ('channel_configs', 'updated_at', true),
              ('channel_categories', 'created_at', true),
              ('channel_categories', 'updated_at', true),
              ('orders', 'created_at', true),
              ('orders', 'paid_at', false),
              ('redemptions', 'created_at', true),
              ('redemptions', 'updated_at', true),
              ('redemptions', 'used_at', false),
              ('redemptions', 'expires_at', false),
              ('redemption_logs', 'created_at', true),
              ('verification_codes', 'created_at', true),
              ('verification_codes', 'expires_at', true),
              ('user_levels', 'created_at', true),
              ('user_levels', 'updated_at', true),
              ('admin_groups', 'created_at', true),
              ('admin_groups', 'updated_at', true),
              ('announcements', 'created_at', true),
              ('announcements', 'updated_at', true),
              ('model_providers', 'created_at', true),
              ('model_providers', 'updated_at', true),
              ('model_types', 'created_at', true),
              ('model_types', 'updated_at', true),
              ('models', 'created_at', true),
              ('models', 'updated_at', true),
              ('model_api_providers', 'created_at', true),
              ('model_api_providers', 'updated_at', true),
              ('forward_rules', 'created_at', true),
              ('forward_rules', 'updated_at', true),
              ('billing_rules', 'created_at', true),
              ('billing_rules', 'updated_at', true),
              ('upstreams', 'created_at', true),
              ('upstreams', 'updated_at', true),
              ('plugins', 'created_at', true),
              ('plugins', 'updated_at', true),
              ('plugin_configs', 'created_at', true),
              ('plugin_configs', 'updated_at', true),
              ('plugin_asset_groups', 'created_at', true),
              ('plugin_asset_groups', 'updated_at', true),
              ('plugin_assets', 'created_at', true),
              ('plugin_assets', 'updated_at', true),
              ('plugin_docs', 'created_at', true),
              ('plugin_docs', 'updated_at', true),
              ('plugin_docs_intl', 'created_at', true),
              ('plugin_docs_intl', 'updated_at', true),
              ('plugin_api_logs', 'created_at', true),
              ('site_icons', 'created_at', true),
              ('site_icons', 'updated_at', true),
              ('site_icon_sync_logs', 'created_at', true),
              ('recharge_records', 'created_at', true),
              ('commissions', 'created_at', true),
              ('playground_projects', 'created_at', true),
              ('playground_projects', 'updated_at', true),
              ('playground_assets', 'created_at', true),
              ('user_model_configs', 'created_at', true),
              ('user_model_configs', 'updated_at', true),
              ('marketing_teams', 'created_at', true),
              ('marketing_teams', 'updated_at', true),
              ('marketing_team_leaders', 'created_at', true),
              ('marketing_team_members', 'created_at', true),
              ('router_flow_groups', 'created_at', true),
              ('router_flow_groups', 'updated_at', true),
              ('tos_temp_files', 'created_at', true),
              ('tos_temp_files', 'expire_at', true),
              ('volcengine_pools', 'created_at', true),
              ('volcengine_pools', 'updated_at', true),
              ('volcengine_pool_accounts', 'created_at', true),
              ('volcengine_pool_accounts', 'updated_at', true),
              ('volcengine_pool_accounts', 'last_error_at', false),
              ('volcengine_pool_logs', 'created_at', true),
              ('gptimage_pools', 'created_at', true),
              ('gptimage_pools', 'updated_at', true),
              ('gptimage_pool_accounts', 'created_at', true),
              ('gptimage_pool_accounts', 'updated_at', true),
              ('gptimage_pool_accounts', 'last_error_at', false),
              ('gptimage_pool_logs', 'created_at', true),
              ('happyhorse_configs', 'created_at', true),
              ('happyhorse_configs', 'updated_at', true),
              ('happyhorse_logs', 'created_at', true),
              ('sys_migration_history', 'executed_at', true)
            ) AS t(tbl, col, nn)
          LOOP
            SELECT EXISTS (
              SELECT 1 FROM information_schema.tables
              WHERE table_schema = 'public' AND table_name = r.tbl
            ) INTO tbl_exists;
            IF NOT tbl_exists THEN
              CONTINUE;
            END IF;

            SELECT (c.data_type IN ('text', 'character varying'))
            INTO is_text
            FROM information_schema.columns c
            WHERE c.table_schema = 'public' AND c.table_name = r.tbl AND c.column_name = r.col;

            IF NOT COALESCE(is_text, false) THEN
              CONTINUE;
            END IF;

            IF r.tbl = 'logs' AND r.col = 'created_at' THEN
              EXECUTE 'DROP INDEX IF EXISTS idx_logs_date_created_at';
            END IF;

            IF r.nn THEN
              ddl := format(
                'ALTER TABLE %I ALTER COLUMN %I DROP DEFAULT, ALTER COLUMN %I TYPE TIMESTAMPTZ USING COALESCE(_tb_text_to_tstz(%I), NOW()), ALTER COLUMN %I SET DEFAULT NOW(), ALTER COLUMN %I SET NOT NULL',
                r.tbl, r.col, r.col, r.col, r.col, r.col
              );
            ELSE
              ddl := format(
                'ALTER TABLE %I ALTER COLUMN %I DROP DEFAULT, ALTER COLUMN %I TYPE TIMESTAMPTZ USING _tb_text_to_tstz(%I)',
                r.tbl, r.col, r.col, r.col
              );
            END IF;
            EXECUTE ddl;
          END LOOP;
        END;
        $mig$"#,
        // 仅清理 TEXT 时代表达式索引；不再同步 CREATE date 索引（会锁大表，且查询多用站点时区桶）。
        "DROP INDEX IF EXISTS idx_logs_date_created_at",
        "DROP FUNCTION IF EXISTS _tb_text_to_tstz(TEXT)"
    );

    // ── logs 冷归档表：热表瘦身；默认不自动归档（log_row_retention_days=0）──
    once_migration!(pool, "logs_archive_v1",
        r#"CREATE TABLE IF NOT EXISTS logs_archive (LIKE logs INCLUDING DEFAULTS)"#,
        r#"DO $$ BEGIN
             ALTER TABLE logs_archive ADD CONSTRAINT logs_archive_pkey PRIMARY KEY (id);
           EXCEPTION WHEN duplicate_object THEN NULL;
           END $$"#,
        "ALTER TABLE logs_archive ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ NOT NULL DEFAULT NOW()",
        "CREATE INDEX IF NOT EXISTS idx_logs_archive_created_at ON logs_archive (created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_logs_archive_user_created ON logs_archive (user_id, created_at DESC)",
        "COMMENT ON TABLE logs_archive IS '使用日志冷归档：超期行从 logs 迁入；仪表盘统计走 usage_daily_stats'"
    );

    // 验证码防爆破：增加 attempts 计数列
    once_migration!(pool, "verification_codes_attempts_v1",
        "ALTER TABLE verification_codes ADD COLUMN IF NOT EXISTS attempts INTEGER NOT NULL DEFAULT 0"
    );

    // ── 查询索引兼容加固（仅 expand：并发补缺 + 清理 INVALID/临时列孤儿索引；不删业务 covering）──
    // 已收口 TIMESTAMPTZ / 已有同名索引的环境可安全重跑；失败不写 history，下次启动重试。
    once_migration!(pool, "query_indexes_compat_v1",
        r#"DO $inv$
        DECLARE r RECORD;
        BEGIN
          FOR r IN
            SELECT c.relname AS idxname
            FROM pg_index i
            JOIN pg_class c ON c.oid = i.indexrelid
            JOIN pg_class t ON t.oid = i.indrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            WHERE n.nspname = 'public'
              AND NOT i.indisvalid
              AND t.relname IN ('logs', 'recharge_records', 'orders', 'users', 'logs_archive')
          LOOP
            EXECUTE format('DROP INDEX IF EXISTS %I', r.idxname);
          END LOOP;
        END
        $inv$"#,
        // expand-contract 临时列索引：列已不存在时清理，避免 planner/维护噪音
        r#"DO $orphan$
        BEGIN
          IF NOT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = 'logs' AND column_name = 'created_at_new'
          ) THEN
            DROP INDEX IF EXISTS idx_logs_user_created_at_new;
            DROP INDEX IF EXISTS idx_logs_created_at_new;
          END IF;
          IF NOT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = 'recharge_records' AND column_name = 'created_at_new'
          ) THEN
            DROP INDEX IF EXISTS idx_recharge_records_user_created_at_new;
            DROP INDEX IF EXISTS idx_recharge_records_created_at_new;
          END IF;
          IF NOT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = 'orders' AND column_name = 'created_at_new'
          ) THEN
            DROP INDEX IF EXISTS idx_orders_user_created_at_new;
            DROP INDEX IF EXISTS idx_orders_created_at_new;
          END IF;
          IF NOT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = 'users' AND column_name = 'created_at_new'
          ) THEN
            DROP INDEX IF EXISTS idx_users_created_at_new;
          END IF;
        END
        $orphan$"#,
        // 表达式索引与业务时区桶不一致，且无查询依赖；并发删除避免锁表
        "DROP INDEX CONCURRENTLY IF EXISTS idx_logs_created_at_date",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_recharge_records_user_created ON recharge_records (user_id, created_at DESC)",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_orders_user_created ON orders (user_id, created_at DESC)",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_users_referred_by ON users (referred_by) WHERE referred_by IS NOT NULL AND referred_by <> ''"
    );

    // 旧 ID 保留（已执行环境跳过）；逻辑已收口到 logs_indexes_reconcile_v1
    once_migration!(pool, "logs_slow_query_indexes_v1", "SELECT 1");
    once_migration!(pool, "logs_created_at_agg_prune_v1", "SELECT 1");

    // ── logs 索引终态（唯一维护点）：确保 agg/vision；尽力删冗余/损坏旧索引 ──
    once_migration!(pool, "logs_indexes_reconcile_v1",
        // 半截并发构建留下的 INVALID：同名 IF NOT EXISTS 会跳过重建，先清掉
        r#"DO $inv$
        DECLARE r RECORD;
        BEGIN
          FOR r IN
            SELECT c.relname AS idxname
            FROM pg_index i
            JOIN pg_class c ON c.oid = i.indexrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname = 'public' AND NOT i.indisvalid
              AND c.relname IN (
                'idx_logs_created_at_agg',
                'idx_logs_vision_created_at_new'
              )
          LOOP
            EXECUTE format('DROP INDEX IF EXISTS %I', r.idxname);
          END LOOP;
        END
        $inv$"#,
        // 日统计半开区间聚合
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_logs_created_at_agg ON logs (created_at ASC)",
        // 视觉深翻页；谓词与 SQL_VISION_ACTION_FILTER 对齐
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_logs_vision_created_at_new ON logs (created_at DESC) WHERE (action_type = ANY (ARRAY['图片'::text, '视频'::text, '视频增强'::text, '视觉模型'::text, '视觉'::text]))",
        // 冗余/expand 残留；lock_timeout 避免与 StartupBackfill 互相堵死（曾导致 DbGate 卡 Checking model）
        r#"DO $prune$
        DECLARE idx text;
        BEGIN
          PERFORM set_config('lock_timeout', '3s', true);
          FOREACH idx IN ARRAY ARRAY[
            'idx_logs_action_created_stats_new',
            'idx_logs_created_at_timestamptz',
            'idx_logs_created_at',
            'idx_logs_user_created_at_new',
            'idx_logs_created_at_new'
          ]
          LOOP
            BEGIN
              EXECUTE format('DROP INDEX IF EXISTS %I', idx);
            EXCEPTION WHEN OTHERS THEN
              RAISE WARNING 'logs_indexes_reconcile_v1 skip drop %: %', idx, SQLERRM;
            END;
          END LOOP;
        END
        $prune$"#,
        "ANALYZE logs"
    );

    // upstream_asset_bindings.is_active 由 INT4 升级为 BIGINT（对齐 Rust i64 与项目其他表约定；幂等）
    once_migration!(pool, "upstream_asset_bindings_is_active_bigint_v1",
        "DO $$ BEGIN IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'upstream_asset_bindings' AND column_name = 'is_active' AND data_type <> 'bigint') THEN ALTER TABLE upstream_asset_bindings ALTER COLUMN is_active TYPE BIGINT; END IF; END $$"
    );

    // 火山视频转素材ID：绑定「上游渠道配置」(channel_configs) + 系统增强插件种子
    once_migration!(pool, "upstream_asset_relay_v1",
        r#"CREATE TABLE IF NOT EXISTS upstream_asset_bindings (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            channel_config_id BIGINT NOT NULL,
            asset_base_path TEXT NOT NULL DEFAULT '',
            forward_rule_id BIGINT,
            group_id TEXT,
            is_active BIGINT NOT NULL DEFAULT 1,
            remark TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_upstream_asset_bindings_config ON upstream_asset_bindings(channel_config_id)",
        "CREATE INDEX IF NOT EXISTS idx_upstream_asset_bindings_rule ON upstream_asset_bindings(forward_rule_id)",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category, allowed_levels, created_at, updated_at)
           VALUES (
             'upstream_asset_relay',
             '火山视频转素材ID',
             '为火山视频任务自动将请求中的媒体 URL 经上游渠道 CreateAsset 转为素材 ID（asset://），并生成可用转发规则',
             0,
             'system',
             'all',
             CURRENT_TIMESTAMP,
             CURRENT_TIMESTAMP
           )
           ON CONFLICT (name) DO UPDATE SET
             title = EXCLUDED.title,
             description = EXCLUDED.description,
             category = EXCLUDED.category"#
    );

    // upstream_asset_bindings 新增 asset_api_profile（协议描述符）列：非火山上游（如 fantaframe/cmcc）
    // 通过声明式描述符在透传分支做双向协议适配；无配置时保持原火山透传行为（幂等）
    // 注意：必须位于建表迁移 upstream_asset_relay_v1 之后，否则空库首启时表尚不存在会失败
    once_migration!(pool, "upstream_asset_bindings_asset_api_profile_v1",
        "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'upstream_asset_bindings' AND column_name = 'asset_api_profile') THEN ALTER TABLE upstream_asset_bindings ADD COLUMN asset_api_profile TEXT; END IF; END $$"
    );

    // ── 模型广场：补齐系统供应商与模型类型英文名称 ──
    once_migration!(pool, "model_marketplace_system_names_en_v1",
        "UPDATE model_providers SET name_en = CASE name WHEN '火山引擎' THEN 'Volcengine' WHEN '谷歌' THEN 'Google' WHEN '阿里云' THEN 'Alibaba Cloud' WHEN '腾讯云' THEN 'Tencent Cloud' WHEN '可灵 AI' THEN 'Kling AI' ELSE name_en END WHERE name_en = ''",
        "UPDATE model_types SET name_en = CASE name WHEN '视频' THEN 'Video' WHEN '图片' THEN 'Image' WHEN '音频' THEN 'Audio' WHEN '聊天' THEN 'Chat' WHEN '向量' THEN 'Embedding' WHEN '排序' THEN 'Rerank' WHEN '视频增强' THEN 'Video Enhancement' ELSE name_en END WHERE name_en = ''"
    );

    // ── 创作中心2026：独立插件注册 + 独立表（与 playground 无共享） ──
    once_migration!(pool, "init_playground_2026_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('playground_2026', '创作中心2026', '提供直接的视频、图片、声音、聊天模型体验服务（2026独立版）', 0, 'user')
           ON CONFLICT (name) DO NOTHING"#,
        r#"CREATE TABLE IF NOT EXISTS playground_2026_projects (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            uid TEXT NOT NULL,
            name TEXT NOT NULL DEFAULT '未命名项目',
            description TEXT DEFAULT '',
            cover_url TEXT DEFAULT '',
            canvas_data TEXT DEFAULT '{}',
            is_deleted INTEGER NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_pg2026_projects_user ON playground_2026_projects(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_projects_uid ON playground_2026_projects(uid)",
        "COMMENT ON TABLE playground_2026_projects IS '创作中心2026项目表（独立于 playground_projects）'",
        r#"CREATE TABLE IF NOT EXISTS playground_2026_assets (
            id BIGSERIAL PRIMARY KEY,
            project_id BIGINT NOT NULL REFERENCES playground_2026_projects(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            uid TEXT NOT NULL,
            asset_type TEXT NOT NULL,
            file_name TEXT DEFAULT '',
            file_size BIGINT DEFAULT 0,
            file_url TEXT NOT NULL,
            tos_object_key TEXT DEFAULT '',
            thumbnail_url TEXT DEFAULT '',
            prompt TEXT DEFAULT '',
            model_id TEXT DEFAULT '',
            model_name TEXT DEFAULT '',
            generation_params TEXT DEFAULT '{}',
            canvas_node_data TEXT DEFAULT '{}',
            duration_seconds DOUBLE PRECISION DEFAULT 0,
            width BIGINT DEFAULT 0,
            height BIGINT DEFAULT 0,
            is_deleted INTEGER NOT NULL DEFAULT 0,
            file_hash TEXT DEFAULT '',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_pg2026_assets_project ON playground_2026_assets(project_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_assets_user ON playground_2026_assets(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_assets_type ON playground_2026_assets(asset_type)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_assets_file_hash ON playground_2026_assets(file_hash)",
        "COMMENT ON TABLE playground_2026_assets IS '创作中心2026素材表（独立于 playground_assets）'",
        r#"CREATE TABLE IF NOT EXISTS user_model_configs_2026 (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            model_mid TEXT NOT NULL,
            param_values TEXT NOT NULL DEFAULT '{}',
            is_locked INTEGER NOT NULL DEFAULT 1,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(user_id, model_mid)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_user_model_configs_2026_user ON user_model_configs_2026(user_id)",
        "COMMENT ON TABLE user_model_configs_2026 IS '用户在创作中心2026锁定的模型自定义参数配置'"
    );

    // ── 创作中心2026：独立令牌限制字段 ──
    once_migration!(pool, "init_playground_2026_token_flag_v1",
        "ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS only_playground_2026 BIGINT NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN api_tokens.only_playground_2026 IS '是否仅限创作中心2026使用，1=是，0=否'"
    );

    // ── 站点门户增强版（商业插件，与 site_portal 配置/静态目录隔离）──
    once_migration!(pool, "init_site_portal_pro_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category)
           VALUES ('site_portal_pro', '站点门户增强版', '提供站点内容的基本介绍，支持生成静态HTML页面用于SEO/GEO优化（商业增强版，与站点门户独立）', 0, 'user')
           ON CONFLICT (name) DO NOTHING"#
    );

    // ── 站点门户：默认改用经典科技风格接管首页（旧默认 apply_to_homepage=false 走托管页）──
    once_migration!(pool, "portal_default_classic_style_homepage_v1",
        r#"UPDATE plugin_configs
           SET config_value = jsonb_set(config_value::jsonb, '{apply_to_homepage}', 'true', true)::text,
               updated_at = CURRENT_TIMESTAMP
           WHERE plugin_name IN ('site_portal', 'site_portal_pro')
             AND config_key = 'style_config'
             AND COALESCE(config_value::jsonb->>'apply_to_homepage', 'false') = 'false'"#,
        r#"UPDATE plugin_configs
           SET config_value = jsonb_set(
                 COALESCE(NULLIF(config_value, '')::jsonb, '{"enabled":false,"html":""}'::jsonb),
                 '{enabled}', 'false', true
               )::text,
               updated_at = CURRENT_TIMESTAMP
           WHERE plugin_name IN ('site_portal', 'site_portal_pro')
             AND config_key = 'custom_homepage'
             AND COALESCE(config_value::jsonb->>'enabled', 'false') = 'true'
             AND COALESCE(TRIM(config_value::jsonb->>'html'), '') = ''"#
    );

    // ── 站点门户增强版独立 DOCS 文档表初始化 ──
    once_migration!(pool, "site_portal_pro_docs_init_v1",
        r#"CREATE TABLE IF NOT EXISTS site_portal_pro_docs (
            id SERIAL PRIMARY KEY,
            parent_id INTEGER NULL REFERENCES site_portal_pro_docs(id) ON DELETE CASCADE,
            title VARCHAR(255) NOT NULL,
            content TEXT DEFAULT '',
            is_dir INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            slug VARCHAR(255) DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS site_portal_pro_docs_intl (
            id SERIAL PRIMARY KEY,
            doc_id INTEGER NOT NULL REFERENCES site_portal_pro_docs(id) ON DELETE CASCADE,
            lang VARCHAR(10) NOT NULL,
            title VARCHAR(255) NOT NULL,
            content TEXT DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text),
            UNIQUE(doc_id, lang)
        )"#
    );

    // ── 站点门户增强版 DOCS 二级分类 ──
    once_migration!(pool, "site_portal_pro_doc_categories_v1",
        r#"CREATE TABLE IF NOT EXISTS site_portal_pro_doc_categories (
            id SERIAL PRIMARY KEY,
            name VARCHAR(100) NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (now()::text),
            updated_at TEXT NOT NULL DEFAULT (now()::text)
        )"#,
        "ALTER TABLE site_portal_pro_docs ADD COLUMN IF NOT EXISTS category_id INTEGER NULL REFERENCES site_portal_pro_doc_categories(id) ON DELETE SET NULL",
        "ALTER TABLE plugin_docs ADD COLUMN IF NOT EXISTS category_id INTEGER NULL",
        r#"INSERT INTO site_portal_pro_doc_categories (name, sort_order)
           SELECT v.name, v.sort_order
           FROM (VALUES
             ('API 参考', 10),
             ('部署安装', 20),
             ('商务支持', 30)
           ) AS v(name, sort_order)
           WHERE NOT EXISTS (SELECT 1 FROM site_portal_pro_doc_categories LIMIT 1)"#,
        r#"UPDATE site_portal_pro_docs d
           SET category_id = c.id
           FROM site_portal_pro_doc_categories c
           WHERE d.parent_id IS NULL
             AND d.category_id IS NULL
             AND c.name = 'API 参考'"#
    );

    // 将仍未归属的根文档挂到「API 参考」（兼容迁移后才导入的旧数据）
    once_migration!(pool, "site_portal_pro_docs_backfill_api_category_v1",
        r#"INSERT INTO site_portal_pro_doc_categories (name, sort_order)
           SELECT 'API 参考', 10
           WHERE NOT EXISTS (
             SELECT 1 FROM site_portal_pro_doc_categories WHERE name = 'API 参考'
           )"#,
        r#"UPDATE site_portal_pro_docs d
           SET category_id = c.id
           FROM site_portal_pro_doc_categories c
           WHERE d.parent_id IS NULL
             AND d.category_id IS NULL
             AND c.name = 'API 参考'"#
    );

    // 合并同名分类（忽略大小写和所有空格），并加唯一约束防止再插入
    once_migration!(pool, "site_portal_pro_doc_categories_dedupe_v1",
        r#"UPDATE site_portal_pro_docs d
           SET category_id = keep.id
           FROM site_portal_pro_doc_categories dup
           JOIN (
             SELECT REPLACE(LOWER(name), ' ', '') AS norm_name, MIN(id) AS id
             FROM site_portal_pro_doc_categories
             GROUP BY REPLACE(LOWER(name), ' ', '')
           ) keep ON REPLACE(LOWER(dup.name), ' ', '') = keep.norm_name
           WHERE d.category_id = dup.id
             AND dup.id <> keep.id"#,
        r#"DELETE FROM site_portal_pro_doc_categories a
           USING site_portal_pro_doc_categories b
           WHERE REPLACE(LOWER(a.name), ' ', '') = REPLACE(LOWER(b.name), ' ', '') AND a.id > b.id"#,
        r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_site_portal_pro_doc_categories_name
           ON site_portal_pro_doc_categories (name)"#
    );

    // 确保 site_portal_pro 「使用指南」分类存在并置为首位 (sort_order = 1)
    once_migration!(pool, "site_portal_pro_doc_categories_user_guide_v2",
        r#"INSERT INTO site_portal_pro_doc_categories (name, sort_order)
           SELECT '使用指南', 1
           WHERE NOT EXISTS (
             SELECT 1 FROM site_portal_pro_doc_categories WHERE name = '使用指南'
           )"#,
        r#"UPDATE site_portal_pro_doc_categories SET sort_order = 1 WHERE name = '使用指南'"#
    );

    // 确保 site_portal_pro 分类包含 is_default 列，默认将「使用指南」置为默认分类 (is_default = 1)
    once_migration!(pool, "site_portal_pro_doc_categories_is_default_v1",
        "ALTER TABLE site_portal_pro_doc_categories ADD COLUMN IF NOT EXISTS is_default INTEGER NOT NULL DEFAULT 0",
        r#"UPDATE site_portal_pro_doc_categories SET is_default = 1 WHERE name = '使用指南' AND NOT EXISTS (SELECT 1 FROM site_portal_pro_doc_categories WHERE is_default = 1)"#
    );

    // 新增「商务合作」分类，并将原挂在 API 参考下的商务合作文档迁入
    once_migration!(pool, "site_portal_pro_doc_categories_business_coop_v1",
        r#"UPDATE site_portal_pro_doc_categories
           SET name = '商务合作', sort_order = 40, updated_at = CURRENT_TIMESTAMP
           WHERE name = '商务支持'
             AND NOT EXISTS (
               SELECT 1 FROM site_portal_pro_doc_categories WHERE name = '商务合作'
             )"#,
        r#"INSERT INTO site_portal_pro_doc_categories (name, sort_order)
           SELECT '商务合作', 40
           WHERE NOT EXISTS (
             SELECT 1 FROM site_portal_pro_doc_categories WHERE name = '商务合作'
           )"#,
        r#"UPDATE site_portal_pro_doc_categories SET sort_order = 40 WHERE name = '商务合作'"#,
        r#"UPDATE site_portal_pro_docs d
           SET category_id = c.id, updated_at = CURRENT_TIMESTAMP
           FROM site_portal_pro_doc_categories c
           WHERE c.name = '商务合作'
             AND d.parent_id IS NULL
             AND (
               d.slug = 'business-cooperation'
               OR REPLACE(LOWER(d.title), ' ', '') IN ('商务合作', 'businesscooperation')
             )"#
    );

    // ── 火山方舟：钱包入账锚点 + 停用原因；删除已废弃的独立 limit_quota ──
    once_migration!(pool, "ark_bindings_wallet_fuse_v1",
        "ALTER TABLE ark_endpoint_bindings ADD COLUMN IF NOT EXISTS wallet_charged_quota DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        "ALTER TABLE ark_endpoint_bindings ADD COLUMN IF NOT EXISTS fuse_reason TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE ark_endpoint_bindings DROP COLUMN IF EXISTS limit_quota",
        "COMMENT ON COLUMN ark_endpoint_bindings.wallet_charged_quota IS '已成功扣入用户钱包的累计消费(元)，与 used_quota 差值即为待扣/待退'",
        "COMMENT ON COLUMN ark_endpoint_bindings.fuse_reason IS '停用原因: wallet=余额熔断(可自动恢复) manual=管理员停用(cron不拉起) 空=正常'",
        // 存量 status=0 视为余额熔断，保证上线后仍可被 cron 自动恢复
        "UPDATE ark_endpoint_bindings SET fuse_reason = 'wallet' WHERE status = 0 AND fuse_reason = ''"
    );

    // ── 级联支持 480p 目标：补默认 res_mul（已有 key 不覆盖）──
    once_migration!(pool, "cascade_480p_target_v1",
        r#"UPDATE forward_rules
           SET config_json = jsonb_set(
             COALESCE(config_json::jsonb, '{}'::jsonb),
             '{res_mul}',
             '{"480p":1.5}'::jsonb || COALESCE(config_json::jsonb -> 'res_mul', '{}'::jsonb),
             true
           )::text
           WHERE COALESCE(config_json::jsonb->>'is_cascade', 'false') IN ('true', '1')
             AND (config_json::jsonb -> 'res_mul' -> '480p') IS NULL"#
    );

    // ── 团队营销：主题推广落地页 ──
    once_migration!(pool, "theme_promotions_v1",
        r#"CREATE TABLE IF NOT EXISTS theme_promotions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            html_content TEXT NOT NULL DEFAULT '',
            status INTEGER NOT NULL DEFAULT 1,
            is_permanent INTEGER NOT NULL DEFAULT 1,
            start_at TIMESTAMPTZ,
            end_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_theme_promotions_status ON theme_promotions (status)",
        "COMMENT ON TABLE theme_promotions IS '高级营销主题推广落地页（HTML 单页活动）'",
        "COMMENT ON COLUMN theme_promotions.slug IS '公开路径段，访问 /promo/{slug}'",
        "COMMENT ON COLUMN theme_promotions.status IS '1=上线 0=下线'",
        "COMMENT ON COLUMN theme_promotions.is_permanent IS '1=长期有效 0=按 start_at/end_at 判断'"
    );

    // ── 主题推广：INTEGER → BIGINT，与全局 i64 约定对齐 ──
    once_migration!(pool, "theme_promotions_bigint_v1",
        "ALTER TABLE theme_promotions ALTER COLUMN status TYPE BIGINT",
        "ALTER TABLE theme_promotions ALTER COLUMN is_permanent TYPE BIGINT"
    );

    // ── 主题推广：新增 promo_type 字段（system=系统推广 custom=自定义推广） ──
    once_migration!(pool, "theme_promotions_promo_type_v1",
        "ALTER TABLE theme_promotions ADD COLUMN IF NOT EXISTS promo_type TEXT NOT NULL DEFAULT 'custom'",
        "COMMENT ON COLUMN theme_promotions.promo_type IS 'system=系统推广(直接跳转首页) custom=自定义推广(单页HTML)'"
    );

    // ── 主题推广：新增 target_path 字段并预置默认系统推广（首页推广 & 模型广场推广） ──
    once_migration!(pool, "theme_promotions_system_defaults_v1",
        "ALTER TABLE theme_promotions ADD COLUMN IF NOT EXISTS target_path TEXT NOT NULL DEFAULT '/'",
        "COMMENT ON COLUMN theme_promotions.target_path IS '系统推广点击后跳转的目标路径'",
        r#"INSERT INTO theme_promotions (id, title, slug, html_content, promo_type, target_path, status, is_permanent)
           VALUES
             ('preset_system_portal', '首页推广', 'portal', '', 'system', '/', 1, 1),
             ('preset_system_models', '模型广场推广', 'models', '', 'system', '/home/models', 1, 1)
           ON CONFLICT (slug) DO UPDATE SET target_path = EXCLUDED.target_path, promo_type = EXCLUDED.promo_type"#
    );

    // 日志 HA 标志请求时快照（读路径勿 JOIN 当前 channels.provider_type）
    once_migration!(pool, "logs_is_ha_snapshot_v1",
        "ALTER TABLE logs ADD COLUMN IF NOT EXISTS is_ha INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE logs_archive ADD COLUMN IF NOT EXISTS is_ha INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN logs.is_ha IS '请求当时是否走高可用组(1=是)；写路径快照，不随渠道配置变更'"
    );

    // ── 营销链接点击统计：同 IP 同日同链接只计 1 次（粗略 UV） ──
    once_migration!(pool, "marketing_link_clicks_v1",
        r#"CREATE TABLE IF NOT EXISTS marketing_link_click_dedup (
            link_type TEXT NOT NULL,
            link_key TEXT NOT NULL,
            promoter_uid TEXT NOT NULL,
            client_ip TEXT NOT NULL,
            click_date DATE NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (link_type, link_key, promoter_uid, client_ip, click_date)
        )"#,
        r#"CREATE TABLE IF NOT EXISTS marketing_link_click_stats (
            link_type TEXT NOT NULL,
            link_key TEXT NOT NULL,
            promoter_uid TEXT NOT NULL,
            click_count BIGINT NOT NULL DEFAULT 0,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (link_type, link_key, promoter_uid)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_mlc_stats_promoter ON marketing_link_click_stats (promoter_uid)",
        "COMMENT ON TABLE marketing_link_click_dedup IS '营销链接点击去重：link_type+link_key+promoter+IP+自然日唯一'",
        "COMMENT ON TABLE marketing_link_click_stats IS '营销链接点击累计（按推广员与链接维度）'",
        "COMMENT ON COLUMN marketing_link_click_dedup.link_type IS 'invite | team_invite | theme_promo'",
        "COMMENT ON COLUMN marketing_link_click_dedup.link_key IS 'invite=_ ; team_invite=邀请码 ; theme_promo=slug'",
        "COMMENT ON COLUMN marketing_link_click_dedup.click_date IS '站点时区下的自然日'"
    );

    // ── 补全预置 ATP Token 视频转发规则与用户等级折扣模式 ──
    once_migration!(pool, "user_level_discount_type_v1",
        r#"INSERT INTO forward_rules (name, rule_type, description, config_json, category, is_system, eid)
        SELECT 'ATP Token 视频生成', 'atp', '将标准视频生成请求（/v1/video/generations）或阿里百炼格式参数转换为 ATP Token 媒体视频 API 格式（omni tasks），支持 Seedance / Kling / Wan / HappyHorse 系列模型，自动轮询及参数兼容', '{"target_type":"atp_video","path_rewrite":{"old":"/v1/video/generations","new":"/omni/media/v1/contents/generations/tasks"},"auth_type":"bearer","poll_path":"/omni/media/v1/contents/generations/tasks/${task_id}"}', '视频', 1, '1' || lpad((floor(random() * 10000)::int)::text, 4, '0')
        WHERE NOT EXISTS (SELECT 1 FROM forward_rules WHERE name = 'ATP Token 视频生成')"#,
        "ALTER TABLE user_levels ADD COLUMN IF NOT EXISTS discount_type INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN user_levels.discount_type IS '折扣模式: 0=不选择(跟随老逻辑全站+等级), 1=使用全站折扣, 2=使用等级折扣'"
    );

    // ── 补全预置 MiniMax 视频生成转发规则（并回填缺失 eid）──
    once_migration!(pool, "minimax_video_forward_rule_v1",
        r#"INSERT INTO forward_rules (name, rule_type, description, config_json, category, is_system, eid)
        SELECT 'MiniMax 视频生成', 'minimax', 'MiniMax V2 视频生成原生通道，自动将多模态请求转换为 prompt 数组，并支持异步任务轮询', '{"target_type":"minimax_video","path_rewrite":{"old":"/v1/video/generations","new":"/v2/video_generation"},"auth_type":"bearer","poll_path":"/v2/query/video_generation/${task_id}"}', '视频', 1, '1' || lpad((floor(random() * 10000)::int)::text, 4, '0')
        WHERE NOT EXISTS (SELECT 1 FROM forward_rules WHERE name = 'MiniMax 视频生成')"#,
        "UPDATE forward_rules SET eid = '1' || lpad((floor(random() * 10000)::int)::text, 4, '0') WHERE eid IS NULL OR eid = ''"
    );

    // ── 数据同步插件：跨站拉取模型目录与计费规则 ──
    once_migration!(pool, "init_data_sync_plugin_v1",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category, allowed_levels, created_at, updated_at)
           VALUES (
             'data_sync',
             '数据同步',
             '通过站点请求密钥跨站拉取模型列表与计费规则；本站优先跳过冲突，不同步折扣与渠道密钥',
             0,
             'system_builtin',
             'all',
             CURRENT_TIMESTAMP,
             CURRENT_TIMESTAMP
           )
           ON CONFLICT (name) DO UPDATE SET
             title = EXCLUDED.title,
             description = EXCLUDED.description,
             category = EXCLUDED.category"#,
        r#"CREATE TABLE IF NOT EXISTS data_sync_logs (
            id BIGSERIAL PRIMARY KEY,
            action TEXT NOT NULL,
            peer_url TEXT,
            operator_id TEXT,
            summary TEXT NOT NULL DEFAULT '{}',
            status TEXT NOT NULL DEFAULT 'success',
            error_message TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_data_sync_logs_created ON data_sync_logs (created_at DESC)",
        "COMMENT ON TABLE data_sync_logs IS '数据同步插件操作审计日志'"
    );

    // ── 数据同步：多站点请求密钥（命名/备注/有效期/IP 白名单）──
    once_migration!(pool, "data_sync_multi_keys_v1",
        r#"CREATE TABLE IF NOT EXISTS data_sync_keys (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            remark TEXT NOT NULL DEFAULT '',
            secret TEXT NOT NULL,
            expires_at TIMESTAMPTZ,
            ip_whitelist TEXT NOT NULL DEFAULT '[]',
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_data_sync_keys_active ON data_sync_keys (is_active)",
        "COMMENT ON TABLE data_sync_keys IS '数据同步站点请求密钥：支持多密钥、备注、有效期、IP白名单(空=不限制)'",
        r#"INSERT INTO data_sync_keys (name, remark, secret, expires_at, ip_whitelist, is_active)
           SELECT '默认密钥', '由旧版单密钥自动迁移', config_value, NULL, '[]', 1
           FROM plugin_configs
           WHERE plugin_name = 'data_sync'
             AND config_key = 'site_request_secret'
             AND COALESCE(config_value, '') <> ''
             AND NOT EXISTS (SELECT 1 FROM data_sync_keys LIMIT 1)"#,
        r#"DELETE FROM plugin_configs
           WHERE plugin_name = 'data_sync' AND config_key = 'site_request_secret'"#
    );

    // ── 上游渠道配置启用/禁用状态 ──
    once_migration!(pool, "channel_configs_status_v1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS status INTEGER NOT NULL DEFAULT 1",
        "COMMENT ON COLUMN channel_configs.status IS '1=启用, 0=禁用'"
    );

    // ── 补全预置 MiniMax 图片生成转发规则 ──
    once_migration!(pool, "minimax_image_forward_rule_v1",
        r#"INSERT INTO forward_rules (name, rule_type, description, config_json, category, is_system, eid)
        SELECT 'MiniMax 图片生成', 'minimax', 'MiniMax 文生图/图生图原生通道（/v1/image_generation），兼容 OpenAI 参数并透传官方字段（aspect_ratio/subject_reference/prompt_optimizer 等）', '{"target_type":"minimax_image","path_rewrite":{"old":"/v1/images/generations","new":"/v1/image_generation"},"auth_type":"bearer"}', '图片', 1, '1' || lpad((floor(random() * 10000)::int)::text, 4, '0')
        WHERE NOT EXISTS (SELECT 1 FROM forward_rules WHERE name = 'MiniMax 图片生成')"#
    );

    // ── MiniMax 规则 rule_type 统一为 minimax（target_type 仍为 minimax_image / minimax_video）──
    once_migration!(pool, "minimax_forward_rule_unify_v1",
        r#"UPDATE forward_rules
        SET rule_type = 'minimax'
        WHERE rule_type IN ('minimax_image', 'minimax_video')"#
    );

    // ── 上游渠道配置分类（复用 channel_categories）──
    once_migration!(pool, "channel_configs_category_v1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS category_id BIGINT REFERENCES channel_categories(id)",
        "COMMENT ON COLUMN channel_configs.category_id IS '上游分类，关联 channel_categories.id'"
    );

    // ── 兑换码：单用户活动参与次数上限（与单码多次兑换解耦）──
    once_migration!(pool, "redemptions_per_user_activity_limit_v1",
        "ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS per_user_activity_limit INTEGER NOT NULL DEFAULT -1",
        "COMMENT ON COLUMN redemptions.per_user_activity_limit IS '同一活动(同 name)下单用户可兑换次数，-1=不限制'",
        "CREATE INDEX IF NOT EXISTS idx_redemptions_name ON redemptions (name)"
    );

    // ── 兑换日志按 user_id 索引：活动参与次数统计 / 防刷查询 ──
    once_migration!(pool, "redemption_logs_user_id_idx_v1",
        "CREATE INDEX IF NOT EXISTS idx_redemption_logs_user_id ON redemption_logs (user_id)"
    );

    // ── 创作中心2026：独立作品表（图片/视频 outputs，与 projects 平级）──
    once_migration!(pool, "init_playground_2026_outputs_v1",
        r#"CREATE TABLE IF NOT EXISTS playground_2026_outputs (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            uid TEXT NOT NULL DEFAULT '',
            media_type TEXT NOT NULL DEFAULT 'image',
            status TEXT NOT NULL DEFAULT 'pending',
            prompt TEXT NOT NULL DEFAULT '',
            model_name TEXT NOT NULL DEFAULT '',
            model_mid TEXT NOT NULL DEFAULT '',
            param_values TEXT NOT NULL DEFAULT '{}',
            preview_url TEXT NOT NULL DEFAULT '',
            aspect_ratio TEXT NOT NULL DEFAULT '',
            resolution TEXT NOT NULL DEFAULT '',
            error_message TEXT NOT NULL DEFAULT '',
            upstream_task_id TEXT NOT NULL DEFAULT '',
            sys_log_id TEXT NOT NULL DEFAULT '',
            batch_id TEXT NOT NULL DEFAULT '',
            is_deleted INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_pg2026_outputs_user_media_created ON playground_2026_outputs(user_id, media_type, created_at DESC)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_outputs_upstream_task ON playground_2026_outputs(upstream_task_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_outputs_batch ON playground_2026_outputs(batch_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_outputs_user_deleted ON playground_2026_outputs(user_id, is_deleted)",
        "COMMENT ON TABLE playground_2026_outputs IS '创作中心2026作品表（图片/视频，独立于 assets/projects）'",
        "COMMENT ON COLUMN playground_2026_outputs.media_type IS 'image|video'",
        "COMMENT ON COLUMN playground_2026_outputs.status IS 'pending|done|error'",
        "COMMENT ON COLUMN playground_2026_outputs.upstream_task_id IS '上游异步任务 id，对应 /v1/tasks/{id}'",
        "COMMENT ON COLUMN playground_2026_outputs.batch_id IS '同一次生成多张结果的批次 id'",
        "COMMENT ON COLUMN playground_2026_outputs.param_values IS '灵活 JSON：方案参数 key→value + _fields[{key,label}] 显示名 + 可选 reference_urls；勿拆成死字段'"
    );

    // ── 创作中心2026：作品分类（收藏夹系统分类 + 用户自定义）──
    once_migration!(pool, "init_playground_2026_albums_v1",
        r#"CREATE TABLE IF NOT EXISTS playground_2026_albums (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            name TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL DEFAULT 'custom',
            is_system INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        r#"CREATE TABLE IF NOT EXISTS playground_2026_album_items (
            album_id BIGINT NOT NULL REFERENCES playground_2026_albums(id) ON DELETE CASCADE,
            output_id BIGINT NOT NULL REFERENCES playground_2026_outputs(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (album_id, output_id)
        )"#,
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_pg2026_albums_user_favorites ON playground_2026_albums(user_id) WHERE kind = 'favorites'",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_albums_user_sort ON playground_2026_albums(user_id, sort_order ASC, id ASC)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_album_items_output ON playground_2026_album_items(output_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_album_items_user ON playground_2026_album_items(user_id, album_id)",
        "COMMENT ON TABLE playground_2026_albums IS '创作中心2026作品分类：favorites 系统收藏夹 + custom 用户分类'",
        "COMMENT ON COLUMN playground_2026_albums.kind IS 'favorites|custom'",
        "COMMENT ON TABLE playground_2026_album_items IS '作品与分类的多对多归属'"
    );

    once_migration!(pool, "pg2026_outputs_param_values_comment_v1",
        "COMMENT ON COLUMN playground_2026_outputs.param_values IS '灵活 JSON：方案参数 key→value + _fields[{key,label}] 显示名 + 可选 reference_urls；勿拆成死字段'"
    );

    // ── 可灵 3.0 推荐转发规则（kling_video，与旧 kling 解耦；文/图一条 + Omni 一条）──
    once_migration!(pool, "kling_video_v3_forward_rules_v1",
        r#"INSERT INTO forward_rules (name, rule_type, description, config_json, category, is_system, eid)
        SELECT * FROM (VALUES
            ('可灵视频 3.0（文/图·推荐）', 'kling_video', '可灵官方 3.0 文生/图生推荐通道：URL 为 /text-to-video/${model}，body 含 contents 时自动改写为 /image-to-video/${model}；统一轮询 /tasks；渠道密钥填官方 API Key（Authorization: Bearer，无需 JWT）', '{"target_type":"kling_video","path_rewrite":{"old":"/v1/video/generations","new":"/text-to-video/${model}"},"auth_type":"bearer","poll_path":"/tasks?task_ids=${task_id}"}', '视频', 1, '1' || lpad((floor(random() * 10000)::int)::text, 4, '0')),
            ('可灵 Omni 视频 3.0（推荐）', 'kling_video', '可灵官方 Omni 3.0 推荐通道：URL 为 /omni-video/${model}；多模态 contents；统一轮询 /tasks；渠道密钥填官方 API Key（Authorization: Bearer，无需 JWT）', '{"target_type":"kling_video","path_rewrite":{"old":"/v1/video/generations","new":"/omni-video/${model}"},"auth_type":"bearer","poll_path":"/tasks?task_ids=${task_id}"}', '视频', 1, '1' || lpad((floor(random() * 10000)::int)::text, 4, '0'))
        ) AS t(name, rule_type, description, config_json, category, is_system, eid)
        WHERE NOT EXISTS (SELECT 1 FROM forward_rules WHERE name = t.name)"#
    );

    // ── 上游渠道配置：日额度自定义刷新时刻 + 冷却分钟 ──
    once_migration!(pool, "channel_configs_daily_reset_cutover_v1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS daily_reset_hour INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS daily_reset_minute INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS daily_reset_cooldown_minutes INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN channel_configs.daily_reset_hour IS '日额度刷新时(0-23)，站点时区'",
        "COMMENT ON COLUMN channel_configs.daily_reset_minute IS '日额度刷新分(0-59)，站点时区'",
        "COMMENT ON COLUMN channel_configs.daily_reset_cooldown_minutes IS '到达刷新时刻后再冷却多少分钟才真正刷新日已用(0=立即)'"
    );

    // ── 火山 MediaKit：插件日志关联表（log_id = logs.id，列表不再全表扫 model）──
    once_migration!(pool, "volcengine_enhance_logs_link_v1",
        r#"CREATE TABLE IF NOT EXISTS volcengine_enhance_logs (
            log_id BIGINT PRIMARY KEY
        )"#,
        "COMMENT ON TABLE volcengine_enhance_logs IS '火山 MediaKit 使用日志关联：log_id=logs.id'",
        "COMMENT ON COLUMN volcengine_enhance_logs.log_id IS '关联主日志表 logs.id'"
    );

    // ── 创作中心2026：工作流（镜像 projects，独立于画布项目）──
    once_migration!(pool, "init_playground_2026_workflows_v1",
        r#"CREATE TABLE IF NOT EXISTS playground_2026_workflows (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            uid TEXT NOT NULL,
            name TEXT NOT NULL DEFAULT '未命名工作流',
            description TEXT DEFAULT '',
            cover_url TEXT DEFAULT '',
            canvas_data TEXT DEFAULT '{}',
            is_deleted INTEGER NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_pg2026_workflows_user ON playground_2026_workflows(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_pg2026_workflows_uid ON playground_2026_workflows(uid)",
        "COMMENT ON TABLE playground_2026_workflows IS '创作中心2026工作流表（节点编排，独立于 playground_2026_projects）'"
    );

    // ── 高可用插件：使用日志（log_id=logs.id，attempts 含全量子渠过程）──
    once_migration!(pool, "ha_usage_logs_v1",
        r#"CREATE TABLE IF NOT EXISTS ha_usage_logs (
            log_id BIGINT PRIMARY KEY,
            group_aid TEXT,
            attempt_count SMALLINT NOT NULL DEFAULT 0,
            final_ok SMALLINT NOT NULL DEFAULT 0,
            final_status_code INT NOT NULL DEFAULT 0,
            attempts JSONB NOT NULL DEFAULT '[]',
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_ha_usage_logs_created ON ha_usage_logs (created_at DESC)",
        "COMMENT ON TABLE ha_usage_logs IS '高可用插件使用日志：log_id=logs.id，attempts=子渠过程JSON'"
    );

    // ── 方舟监控：流水统计起点 + 热点查询索引 ──
    once_migration!(pool, "ark_monitor_ledger_after_and_indexes_v1",
        "ALTER TABLE ark_endpoint_bindings ADD COLUMN IF NOT EXISTS wallet_ledger_after TIMESTAMPTZ",
        "COMMENT ON COLUMN ark_endpoint_bindings.wallet_ledger_after IS '方舟钱包流水统计起点：换绑用户/接入点时置为当前时间；NULL=统计全部历史流水'",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_ark_bindings_account ON ark_endpoint_bindings (account_id)",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_ark_video_tasks_ep_estimated ON ark_video_tasks (endpoint_id) WHERE is_estimated = TRUE AND status IN ('succeeded', 'success')",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_ark_video_tasks_ep_confirmed ON ark_video_tasks (endpoint_id) WHERE is_estimated = FALSE AND status IN ('succeeded', 'success')",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_recharge_ark_monitor_user_created ON recharge_records (user_id, created_at) WHERE operator = 'ark_monitor' AND recharge_type IN ('ark_video_consume', 'ark_video_refund')"
    );

    // ── 用户实名认证 KYC ──
    once_migration!(pool, "user_kyc_v1",
        r#"CREATE TABLE IF NOT EXISTS user_kyc (
            id BIGSERIAL PRIMARY KEY,
            user_id TEXT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
            kyc_type TEXT NOT NULL DEFAULT 'personal',
            status TEXT NOT NULL DEFAULT 'none',
            real_name TEXT,
            id_doc_type TEXT,
            id_doc_front_url TEXT,
            id_doc_back_url TEXT,
            company_name TEXT,
            business_license_url TEXT,
            tax_registration_url TEXT,
            legal_notarization_url TEXT,
            validity_type TEXT NOT NULL DEFAULT 'long_term',
            expire_at TIMESTAMPTZ,
            reject_reason TEXT,
            admin_remark TEXT,
            reviewed_by TEXT,
            reviewed_at TIMESTAMPTZ,
            submitted_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_user_kyc_status ON user_kyc(status)",
        "COMMENT ON TABLE user_kyc IS '用户实名认证：个人/企业证件与有效期'",
        "COMMENT ON COLUMN user_kyc.kyc_type IS 'personal|enterprise'",
        "COMMENT ON COLUMN user_kyc.status IS 'none|pending|approved|rejected|expired'",
        "COMMENT ON COLUMN user_kyc.id_doc_type IS 'id_card|passport|driver_license'",
        "COMMENT ON COLUMN user_kyc.validity_type IS 'long_term|expire_date'"
    );

    // ── 新增模型：折扣限价默认开启，倍率默认 1.0 ──
    once_migration!(pool, "models_site_discount_default_on_v1",
        "ALTER TABLE models ALTER COLUMN site_discount SET DEFAULT 1.0",
        "ALTER TABLE models ALTER COLUMN site_discount_enabled SET DEFAULT 1",
        "COMMENT ON COLUMN models.site_discount IS '折扣限价倍率（开启时折扣不低于此值，默认 1.0=原价）'",
        "COMMENT ON COLUMN models.site_discount_enabled IS '折扣限价开关（0=关，1=开，新增默认开启）'"
    );

    // ── 更新 Seedance 2.0 官方计费规则（PID 74112）系统默认配置（包含 4K 分辨率计费） ──
    once_migration!(pool, "update_seedance2_0_default_rule_4k_v1",
        r#"UPDATE billing_rules SET extended_config = '{"resolution_rates":{"1080p":{"with_video":31,"without_video":51},"480p":{"with_video":28,"without_video":46},"4k":{"with_video":16,"without_video":26},"720p":{"with_video":28,"without_video":46}}}' WHERE (name = 'Seedance2.0官方计费' OR pid = '74112') AND is_system = 1"#,
        "UPDATE billing_rules SET pid = '74112' WHERE name = 'Seedance2.0官方计费' AND is_system = 1 AND (pid = '' OR pid IS NULL)"
    );

    // ── 将 Seedance 2.5 官方计费规则调整为系统计费规则 ──
    once_migration!(pool, "make_seedance2_5_system_rule_v1",
        "UPDATE billing_rules SET is_system = 1, pid = CASE WHEN pid LIKE '6%' OR pid = '' OR pid IS NULL THEN '73119' ELSE pid END WHERE name = 'Seedance2.5官方计费'",
        r#"INSERT INTO billing_rules (name, billing_type, prompt_rate, completion_rate, fixed_rate, duration_rate, billing_rule, extended_config, is_system, pid, pricing_type)
        SELECT 'Seedance2.5官方计费', 'tokens', 0.0, 0.0, 0.0, 0.0, 'seedance2.0', '{"enable_time_multipliers":false,"resolution_rates":{"480p":{"with_video":42,"without_video":70},"720p":{"with_video":42,"without_video":70}},"time_multipliers":[]}', 1, '73119', 'official'
        WHERE NOT EXISTS (SELECT 1 FROM billing_rules WHERE name = 'Seedance2.5官方计费')
        "#
    );

    // ComfyUI 接入：服务/工作流/任务表 + 系统增强插件种子（转发规则由插件运行时生成，不预置）
    once_migration!(pool, "comfyui_bridge_v1",
        r#"CREATE TABLE IF NOT EXISTS comfyui_servers (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            base_url TEXT NOT NULL,
            auth_header TEXT NOT NULL DEFAULT '',
            timeout_secs INTEGER NOT NULL DEFAULT 120,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        r#"CREATE TABLE IF NOT EXISTS comfyui_workflows (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            server_id BIGINT NOT NULL,
            workflow_json TEXT NOT NULL DEFAULT '{}',
            prompt_template TEXT NOT NULL DEFAULT '',
            param_map TEXT NOT NULL DEFAULT '{}',
            output_node_id TEXT NOT NULL DEFAULT '',
            forward_rule_id BIGINT,
            is_active INTEGER NOT NULL DEFAULT 1,
            remark TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_comfyui_workflows_server ON comfyui_workflows(server_id)",
        "CREATE INDEX IF NOT EXISTS idx_comfyui_workflows_rule ON comfyui_workflows(forward_rule_id)",
        r#"CREATE TABLE IF NOT EXISTS comfyui_jobs (
            log_id BIGINT PRIMARY KEY,
            prompt_id TEXT NOT NULL,
            workflow_id BIGINT NOT NULL,
            server_id BIGINT NOT NULL,
            output_url TEXT
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_comfyui_jobs_prompt ON comfyui_jobs(prompt_id)",
        "COMMENT ON TABLE comfyui_jobs IS 'ComfyUI 任务：log_id=logs.id'",
        "COMMENT ON COLUMN comfyui_jobs.log_id IS '关联主日志表 logs.id'",
        r#"INSERT INTO plugins (name, title, description, is_enabled, category, allowed_levels, created_at, updated_at)
           VALUES (
             'comfyui_bridge',
             'ComfyUI 接入',
             '管理 ComfyUI 服务地址与工作流，经 OpenAI 视频路由提交并轮询生成结果',
             0,
             'system',
             'all',
             CURRENT_TIMESTAMP,
             CURRENT_TIMESTAMP
           )
           ON CONFLICT (name) DO UPDATE SET
             title = EXCLUDED.title,
             description = EXCLUDED.description,
             category = EXCLUDED.category"#
    );

    // 工作流 ↔ 服务节点多对多；旧 server_id 回填后改为可空
    once_migration!(pool, "comfyui_workflow_nodes_v1",
        r#"CREATE TABLE IF NOT EXISTS comfyui_workflow_nodes (
            workflow_id BIGINT NOT NULL,
            server_id BIGINT NOT NULL,
            PRIMARY KEY (workflow_id, server_id)
        )"#,
        "CREATE INDEX IF NOT EXISTS idx_comfyui_wf_nodes_server ON comfyui_workflow_nodes(server_id)",
        r#"INSERT INTO comfyui_workflow_nodes (workflow_id, server_id)
           SELECT id, server_id FROM comfyui_workflows
           WHERE server_id IS NOT NULL
           ON CONFLICT DO NOTHING"#,
        "ALTER TABLE comfyui_workflows ALTER COLUMN server_id DROP NOT NULL"
    );

    once_migration!(pool, "comfyui_dispatch_v1",
        "ALTER TABLE comfyui_servers ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE comfyui_servers ADD COLUMN IF NOT EXISTS weight INTEGER NOT NULL DEFAULT 1",
        "ALTER TABLE comfyui_servers ADD COLUMN IF NOT EXISTS sort_order INTEGER NOT NULL DEFAULT 0",
        r#"CREATE TABLE IF NOT EXISTS comfyui_dispatch_rules (
            id BIGSERIAL PRIMARY KEY,
            code TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            remark TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        )"#,
        r#"INSERT INTO comfyui_dispatch_rules (code, name, remark, is_active, sort_order)
           VALUES
             ('priority_weight', '权重优先', '先取优先级最高的节点，同分再按权重随机', 1, 1),
             ('random', '随机调用', '在已选且启用的节点中均匀随机', 1, 2),
             ('sequential', '顺序调用', '按节点排序依次轮流', 1, 3),
             ('least_busy', '空闲优先', '未完成任务最少的节点优先，同分再按权重优先', 1, 4)
           ON CONFLICT (code) DO NOTHING"#
    );

    // ── 模型来源：系统预设 / 自定义 ──
    once_migration!(pool, "models_is_system_v1",
        "ALTER TABLE models ADD COLUMN IF NOT EXISTS is_system INTEGER NOT NULL DEFAULT 0",
        "COMMENT ON COLUMN models.is_system IS '1=系统预设，0=自定义'",
        "UPDATE models SET is_system = 1 WHERE mid IN ('vve-sd', 'vve-pf', 'vve-ft', 'vve-gt', 'vvs-er', 'vvs-ep', 'dbs-sr', 'dbs-fs')"
    );

    // ── 安装时写入系统预设模型（官方计费 + 转发规则）──
    let preset_models_done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sys_migration_history WHERE id = 'seed_system_preset_models_v1'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    if preset_models_done == 0 {
        match crate::db::preset_models::seed_system_preset_models(pool).await {
            Ok(n) => {
                let _ = sqlx::query(
                    "INSERT INTO sys_migration_history (id) VALUES ('seed_system_preset_models_v1')",
                )
                .execute(pool)
                .await;
                tracing::info!("系统预设模型种子写入完成，新增 {} 条", n);
            }
            Err(e) => {
                tracing::error!("seed_system_preset_models_v1 失败，未标记完成以便重试: {e}");
            }
        }
    }

    once_migration!(pool, "channel_configs_upstream_rate_sync_v1",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS upstream_system TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS upstream_group TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS upstream_sync_interval_minutes INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS upstream_sync_rate_add DOUBLE PRECISION NOT NULL DEFAULT 0",
        "ALTER TABLE channel_configs ADD COLUMN IF NOT EXISTS upstream_synced_at TIMESTAMPTZ",
        "COMMENT ON COLUMN channel_configs.upstream_system IS '上游系统: 兼容/官方/newapi/akeapi/火山引擎/阿里云，空=未选'",
        "COMMENT ON COLUMN channel_configs.upstream_group IS 'NewAPI 等已选同步分组名'",
        "COMMENT ON COLUMN channel_configs.upstream_sync_interval_minutes IS '分组倍率自动同步间隔分钟，0=关闭'",
        "COMMENT ON COLUMN channel_configs.upstream_sync_rate_add IS '同步时叠加到分组倍率上的增量，0=不叠加'",
        "COMMENT ON COLUMN channel_configs.upstream_synced_at IS '上次成功同步分组倍率的时间'"
    );

    // task_id 按 id 倒序取最新一行；pending 轮询；旧单列索引由复合索引覆盖
    once_migration!(pool, "logs_task_id_id_pending_poll_idx_v1",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_logs_task_id_id ON logs (task_id, id DESC)",
        "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_logs_pending_poll ON logs (id ASC) WHERE is_completed = 0 AND status_code = 200",
        "DROP INDEX CONCURRENTLY IF EXISTS idx_logs_task_id"
    );

    // 菜单配置：将 /wallet 的 label_zh 从「资产中心」更新为「我的钱包」
    once_migration!(pool, "update_menu_wallet_label_to_my_wallet_20260815",
        "UPDATE settings SET value = replace(
            replace(value, '\"label_zh\":\"资产中心\"', '\"label_zh\":\"我的钱包\"'),
            '\"label_zh\": \"资产中心\"', '\"label_zh\": \"我的钱包\"'
        ) WHERE key = 'menu_config_settings'"
    );

    // 站点设置：版权信息默认值对齐为「© 2026 TkeAPI. All rights reserved.」
    once_migration!(pool, "update_site_copyright_default_20260815",
        "UPDATE settings SET value = replace(
            replace(
                replace(
                    replace(value, '\"copyright\":\"© 2026 Tkeapi. All rights reserved.\"', '\"copyright\":\"© 2026 TkeAPI. All rights reserved.\"'),
                    '\"copyright\": \"© 2026 Tkeapi. All rights reserved.\"', '\"copyright\": \"© 2026 TkeAPI. All rights reserved.\"'
                ),
                '\"copyright\":\"© 2026 MyCompany. All rights reserved.\"', '\"copyright\":\"© 2026 TkeAPI. All rights reserved.\"'
            ),
            '\"copyright\": \"© 2026 MyCompany. All rights reserved.\"', '\"copyright\": \"© 2026 TkeAPI. All rights reserved.\"'
        ) WHERE key = 'site_settings'"
    );

    tracing::info!("PostgreSQL AnyPool migrations completed successfully");
    Ok(())
    }};
}

pub async fn run_pg(pool: &sqlx::Pool<sqlx::Postgres>) -> anyhow::Result<()> {
    pg_migration_blocks!(pool)
}
