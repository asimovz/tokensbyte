/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 * @copyright      Copyright netbcloud/wstianxia
 * @license        MIT (https://www.tokensbyte.ai/)
 */

use crate::middleware::{admin_middleware, api_key_middleware, auth_middleware};
use crate::AppState;
use axum::{
    middleware as axum_middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;

pub mod admin_groups;
pub mod admin_stats_sync;
pub mod announcements;
pub mod auth;
pub mod billing_rules;
pub mod channel_categories;
pub mod channel_configs;
pub mod channels;
pub mod dashboard;
pub mod date_helper;
pub mod forward_rules;
pub mod logs;
pub mod metrics;
pub mod model_classifications;
pub mod models;
/// All backend plugins (Playground / Docs / Site / Commercial / …)
pub mod plugins;
pub mod settings;
pub mod task_logs;
pub mod tokens;
pub mod upstream_asset_bindings;
pub mod upstreams;
pub mod user;
pub mod user_kyc;
pub mod user_levels;
pub mod users;

pub fn build_router(state: Arc<AppState>) -> Router {
    // 1. Management APIs (Admin/User UI)
    let mut admin_routes: Router<Arc<AppState>> = Router::new()
        .route("/users", get(users::list_users).post(users::create_user))
        .route(
            "/users/consumption/stats_batch",
            post(users::get_consumption_stats_batch),
        )
        .route(
            "/users/{id}",
            put(users::update_user).delete(users::delete_user),
        )
        .route("/users/{id}/recharge", post(users::recharge_user))
        .route("/users/{id}/impersonate", post(users::impersonate_user))
        .route("/users/{id}/level-logs", get(users::get_user_level_logs))
        .route(
            "/users/{id}/kyc",
            get(user_kyc::admin_get_user_kyc).put(user_kyc::admin_upsert_user_kyc),
        )
        .route("/channels", post(channels::create_channel))
        .route(
            "/channels/{id}",
            put(channels::update_channel).delete(channels::delete_channel),
        )
        .route("/channels/{id}/test", post(channels::test_channel))
        .route(
            "/channels/{id}/meltdown",
            get(channels::get_meltdown_status),
        )
        .route(
            "/channels/{id}/meltdown/reset",
            post(channels::reset_meltdown),
        )
        .route("/channels/{id}/quota/reset", post(channels::reset_quota))
        .route(
            "/channel-categories",
            get(channel_categories::list_categories).post(channel_categories::create_category),
        )
        .route(
            "/channel-categories/{id}",
            put(channel_categories::update_category).delete(channel_categories::delete_category),
        )
        .route(
            "/channel-configs",
            post(channel_configs::create_channel_config),
        )
        .route(
            "/channel-configs/upstream-groups",
            post(channel_configs::fetch_upstream_groups),
        )
        .route(
            "/channel-configs/{id}",
            put(channel_configs::update_channel_config)
                .delete(channel_configs::delete_channel_config),
        )
        .route(
            "/channel-configs/{id}/quota/reset",
            post(channel_configs::reset_quota),
        )
        .route(
            "/upstream-asset-bindings",
            post(upstream_asset_bindings::create_binding),
        )
        .route(
            "/upstream-asset-bindings/{id}",
            put(upstream_asset_bindings::update_binding)
                .delete(upstream_asset_bindings::delete_binding),
        )
        .route(
            "/upstream-asset-bindings/{id}/test",
            post(upstream_asset_bindings::test_binding),
        )
        .route(
            "/upstreams",
            get(upstreams::list_upstreams).post(upstreams::create_upstream),
        )
        .route(
            "/upstreams/{id}",
            put(upstreams::update_upstream).delete(upstreams::delete_upstream),
        )
        .route(
            "/upstreams/{id}/balance",
            get(upstreams::get_upstream_balance),
        )
        .route("/models", post(models::create_model))
        .route(
            "/models/{id}",
            put(models::update_model).delete(models::delete_model),
        )
        .route(
            "/model-providers",
            get(model_classifications::list_providers).post(model_classifications::create_provider),
        )
        .route(
            "/model-providers/{id}",
            put(model_classifications::update_provider)
                .delete(model_classifications::delete_provider),
        )
        .route(
            "/model-api-providers",
            get(model_classifications::list_api_providers)
                .post(model_classifications::create_api_provider),
        )
        .route(
            "/model-api-providers/{id}",
            put(model_classifications::update_api_provider)
                .delete(model_classifications::delete_api_provider),
        )
        .route(
            "/model-types",
            get(model_classifications::list_types).post(model_classifications::create_type),
        )
        .route(
            "/model-types/{id}",
            put(model_classifications::update_type).delete(model_classifications::delete_type),
        )
        .route(
            "/classifications/stats",
            get(model_classifications::get_classifications_stats),
        );

    #[cfg(plugin_redemptions)]
    {
        admin_routes = admin_routes
            .route(
                "/redemptions",
                get(plugins::redemptions::list_redemptions)
                    .post(plugins::redemptions::generate_redemptions),
            )
            .route(
                "/redemptions/groups",
                get(plugins::redemptions::list_redemption_groups)
                    .delete(plugins::redemptions::delete_redemption_group),
            )
            .route(
                "/redemptions/{id}",
                delete(plugins::redemptions::delete_redemption),
            )
            .route(
                "/redemptions/{id}/status",
                put(plugins::redemptions::update_redemption_status),
            );
    }

    admin_routes = admin_routes
        .route("/tokens/all", get(tokens::list_all_tokens))
        .route("/settings", post(settings::update_settings))
        .route("/settings/full", get(settings::get_settings))
        .route(
            "/settings/usage-stats/sync",
            post(admin_stats_sync::trigger_stats_sync),
        )
        .route("/settings/database/verify", post(settings::verify_database))
        .route("/settings/database/info", get(settings::database_info))
        .route(
            "/settings/database/initialize",
            post(settings::initialize_database),
        )
        .route("/settings/database/backup", post(settings::backup_database))
        .route(
            "/settings/storage/test",
            post(settings::test_storage_connection),
        )
        .route("/settings/email/test", post(settings::test_email))
        .route("/settings/sms/test", post(settings::test_sms))
        .route(
            "/settings/notification/test",
            post(settings::test_low_balance_notification),
        )
        .route("/settings/repair-logs", post(settings::repair_failed_logs))
        .route(
            "/user_levels",
            get(user_levels::list_user_levels).post(user_levels::create_user_level),
        )
        .route(
            "/user_levels/{id}",
            put(user_levels::update_user_level).delete(user_levels::delete_user_level),
        );

    #[cfg(plugin_finance)]
    {
        admin_routes = admin_routes
            .route("/finance/orders", get(plugins::finance::list_orders))
            .route("/finance/recharges", get(plugins::finance::list_recharges))
            .route(
                "/finance/recharges/stats_batch",
                post(plugins::finance::get_wallet_stats_batch),
            )
            .route(
                "/finance/recharge_types",
                get(plugins::finance::list_recharge_types),
            )
            .route(
                "/finance/daily-stats",
                get(plugins::finance::get_daily_stats),
            );
    }

    let admin_routes = admin_routes
        .route(
            "/admin_groups",
            get(admin_groups::list_admin_groups).post(admin_groups::create_admin_group),
        )
        .route(
            "/admin_groups/{id}",
            put(admin_groups::update_admin_group).delete(admin_groups::delete_admin_group),
        )
        .route(
            "/forward-rules",
            get(forward_rules::list_rules).post(forward_rules::create_rule),
        )
        .route(
            "/forward-rules/{id}",
            put(forward_rules::update_rule).delete(forward_rules::delete_rule),
        )
        .route(
            "/billing-rules",
            get(billing_rules::list_rules).post(billing_rules::create_rule),
        )
        .route(
            "/billing-rules/{id}",
            put(billing_rules::update_rule).delete(billing_rules::delete_rule),
        )
        .route(
            "/billing-rules/{id}/restore-default",
            post(billing_rules::restore_default_rule),
        )
        .route(
            "/announcements",
            get(announcements::list_admin_announcements).post(announcements::create_announcement),
        )
        .route(
            "/announcements/{id}",
            put(announcements::update_announcement).delete(announcements::delete_announcement),
        )
        .layer(axum_middleware::from_fn(admin_middleware))
        .with_state(state.clone());

    let mut management_routes: Router<Arc<AppState>> = Router::new()
        .route("/dashboard", get(dashboard::get_stats))
        .route("/dashboard/models_30d", get(dashboard::get_model_stats_30d))
        .route("/metrics/live", get(metrics::get_live_metrics))
        .route("/channels", get(channels::list_channels))
        .route("/models", get(models::list_models))
        .route(
            "/tokens",
            get(tokens::list_tokens).post(tokens::create_token),
        )
        .route(
            "/tokens/{id}",
            put(tokens::update_token).delete(tokens::delete_token),
        )
        .route("/tokens/{id}/reveal", post(tokens::reveal_token))
        .route("/tokens/{id}/reset-usage", post(tokens::reset_token_usage))
        .route(
            "/channel-configs",
            get(channel_configs::list_channel_configs),
        )
        .route(
            "/upstream-asset-bindings",
            get(upstream_asset_bindings::list_bindings),
        )
        .route("/logs", get(logs::list_logs))
        .route("/logs/export", get(logs::export_logs))
        .route("/logs/{id}/detail", get(logs::get_log_detail));

    #[cfg(plugin_redemptions)]
    {
        management_routes = management_routes.route(
            "/redemptions/redeem",
            post(plugins::redemptions::redeem_code),
        );
    }

    management_routes = management_routes
        .route(
            "/user/profile",
            get(user::get_profile).put(user::update_profile),
        )
        .route("/user/wallet", get(user::get_wallet_stats))
        .route("/user/recharge_records", get(user::list_recharge_records))
        .route("/user/affiliate/transfer", post(user::transfer_commission))
        .route("/user/bind/mobile", post(user::bind_mobile))
        .route("/user/bind/email", post(user::bind_email))
        .route("/user/bind/oauth-state", get(user::bind_oauth_state))
        .route("/user/bind/wechat", get(user::bind_wechat))
        .route("/user/bind/google", get(user::bind_google))
        .route("/user/unbind/{bind_type}", post(user::unbind_third_party))
        .route(
            "/user/kyc",
            get(user_kyc::get_my_kyc).put(user_kyc::submit_my_kyc),
        )
        .route("/user/kyc/upload", post(user_kyc::upload_kyc_document))
        .route("/task_logs", get(task_logs::list_task_logs))
        .route("/task_logs/{log_id}/sync", post(task_logs::sync_task_log))
        .route(
            "/task_logs/{log_id}/cancel",
            post(task_logs::cancel_task_log),
        )
        .route("/task_logs/export", get(task_logs::export_task_logs));

    #[cfg(all(plugin_pay, plugin_payment))]
    {
        management_routes = management_routes
            .route("/finance/pay/create", post(plugins::pay::create_order))
            .route(
                "/finance/pay/status/{out_trade_no}",
                get(plugins::pay::check_status),
            );
    }

    management_routes = management_routes
        .route("/system/about", get(settings::system_about))
        .merge(admin_routes);

    {
        management_routes = management_routes.nest("/plugins", plugins::router());
    }
    #[cfg(feature = "commercial_plugins")]
    {
        management_routes = management_routes.nest("/assets", plugins::assets::router());
    }

    #[cfg(feature = "commercial_plugins")]
    #[cfg(plugin_team_marketing)]
    {
        management_routes =
            management_routes.nest("/team-marketing", plugins::team_marketing::router());
    }

    let management_routes = management_routes.nest("/playground", plugins::playground::router());
    #[cfg(feature = "commercial_plugins")]
    let management_routes =
        management_routes.nest("/playground-2026", plugins::playground_2026::router());

    #[cfg(feature = "plugin_site_icons")]
    let management_routes =
        management_routes.nest("/plugins/site-icons", plugins::site_icons::router());

    #[cfg(feature = "plugin_site_portal")]
    let management_routes =
        management_routes.nest("/plugins/site-portal", plugins::site_portal::admin_router());
    #[cfg(feature = "commercial_plugins")]
    let management_routes = management_routes.nest(
        "/plugins/site-portal-pro",
        plugins::site_portal_pro::admin_router(),
    );
    #[cfg(feature = "plugin_happyhorse")]
    let management_routes = management_routes.nest(
        "/plugins/happyhorse_router",
        plugins::happyhorse_router::router(),
    );
    #[cfg(feature = "plugin_comfyui")]
    let management_routes = management_routes.nest(
        "/plugins/comfyui_bridge",
        plugins::comfyui_bridge::router(),
    );
    let management_routes =
        management_routes.nest("/plugins/docs-api", plugins::docs_api::router());
    #[cfg(feature = "commercial_plugins")]
    let management_routes = management_routes.nest(
        "/plugins/volcengine_ark_monitor",
        plugins::volc_ark_monitor::router(),
    );
    #[cfg(feature = "commercial_plugins")]
    let management_routes = management_routes.nest(
        "/plugins/upstream_asset_relay",
        plugins::upstream_asset_relay::router(),
    );
    #[cfg(feature = "plugin_data_sync")]
    let management_routes =
        management_routes.nest("/plugins/data_sync", plugins::data_sync::router());
    let management_routes = management_routes.route_layer(axum_middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));

    let mut payment_public_routes: Router<Arc<AppState>> = Router::new();
    #[cfg(all(plugin_pay, plugin_payment))]
    {
        payment_public_routes = payment_public_routes
            .route(
                "/finance/pay/notify/wechat",
                post(plugins::pay::wechat_notify),
            )
            .route(
                "/finance/pay/notify/alipay",
                post(plugins::pay::alipay_notify),
            )
            .route(
                "/finance/pay/notify/stripe",
                post(plugins::pay::stripe_notify),
            )
            .route(
                "/finance/pay/notify/bonuspay",
                post(plugins::pay::bonuspay_notify),
            )
            .route(
                "/finance/pay/notify/hyperbc",
                post(plugins::pay::hyperbc_notify),
            )
            .route(
                "/finance/pay/notify/allinpay",
                post(plugins::pay::allinpay_notify),
            );
    }

    let payment_public_routes = payment_public_routes.with_state(state.clone());

    // 2. Auth APIs & Public Configs (Public)
    let auth_routes: Router<Arc<AppState>> = Router::new()
        .route("/login", post(auth::login))
        .route("/admin/login", post(auth::admin_login))
        .route("/exchange-login-code", post(auth::exchange_login_code))
        .route("/admin/init-status", get(auth::admin_init_status))
        .route("/admin/init", post(auth::init_admin))
        .route("/register", post(auth::register))
        .route("/send-code", post(auth::send_code))
        .route("/send-sms-code", post(auth::send_sms_code))
        .route("/register-email", post(auth::register_email))
        .route("/register-mobile", post(auth::register_mobile))
        .route("/reset-password", post(auth::reset_password))
        .route("/oauth/state", get(auth::oauth_state))
        .route("/oauth/wechat", get(auth::oauth_wechat))
        .route("/oauth/wechat/callback", get(auth::oauth_wechat_callback))
        .route("/oauth/google", get(auth::oauth_google))
        .route("/oauth/google/callback", get(auth::oauth_google_callback))
        .with_state(state.clone());

    let mut public_v1_routes: Router<Arc<AppState>> = Router::new()
        .route("/settings", get(settings::get_public_settings))
        .route(
            "/announcements/public",
            get(announcements::get_public_announcements),
        )
        .route("/plugins/active", get(plugins::get_active_plugins_public))
        .route(
            "/plugins/docs-api/public/tree",
            get(plugins::docs_api::list_docs_public),
        )
        .route(
            "/plugins/docs-api/public/docs/{id}",
            get(plugins::docs_api::get_doc_detail),
        )
        // OAuth 绑定回调（浏览器重定向，无 JWT，通过 state 参数识别用户）
        .route(
            "/user/bind/wechat/callback",
            get(user::bind_wechat_callback),
        )
        .route(
            "/user/bind/google/callback",
            get(user::bind_google_callback),
        )
        // 厂商任务回调（无 JWT；上游推送后结案并回传客户端原 callback_url）
        .route(
            "/relay/vendor-callback/{id}",
            post(crate::relay::vendor_callback::vendor_callback),
        )
        .merge(payment_public_routes);
    {
        public_v1_routes =
            public_v1_routes.route("/marketplace/public", get(plugins::get_marketplace_public));
    }
    #[cfg(feature = "commercial_plugins")]
    {
        public_v1_routes = public_v1_routes
            .route(
                "/plugins/site-portal-pro/public/tree",
                get(plugins::site_portal_pro_docs::list_docs_public),
            )
            .route(
                "/plugins/site-portal-pro/public/docs/categories",
                get(plugins::site_portal_pro_docs::list_categories_public),
            )
            .route(
                "/plugins/site-portal-pro/public/docs/{id}",
                get(plugins::site_portal_pro_docs::get_doc_detail_public),
            );
    }
    #[cfg(feature = "commercial_plugins")]
    #[cfg(plugin_team_marketing)]
    {
        public_v1_routes = public_v1_routes
            .route(
                "/team-marketing/theme-promos/public/{slug}",
                get(plugins::team_marketing::theme_promo::get_theme_promo_public),
            )
            .route(
                "/team-marketing/link-clicks",
                post(plugins::team_marketing::link_click::record_link_click),
            );
    }
    #[cfg(feature = "plugin_data_sync")]
    {
        public_v1_routes =
            public_v1_routes.nest("/plugins/data_sync", plugins::data_sync::public_router());
    }
    let public_v1_routes = public_v1_routes.with_state(state.clone());

    // 3. Relay APIs (OpenAI Compatible + 可灵原生)
    //    统一挂载在 /v1 前缀下，路由定义为相对路径
    let relay_routes: Router<Arc<AppState>> = Router::new()
        .route(
            "/chat/completions",
            post(crate::relay::chat::chat_completions),
        )
        .route("/messages", post(crate::relay::chat::chat_completions))
        .route("/responses", post(crate::relay::chat::responses_create))
        .route(
            "/images/generations",
            post(crate::relay::image::image_generations),
        )
        .route(
            "/images/edits",
            post(crate::relay::image::image_generations),
        )
        .route(
            "/video/generations",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/video/generations/{task_id}",
            get(crate::relay::task::task_status),
        )
        .route("/tasks/{task_id}", get(crate::relay::task::task_status))
        // 可灵 AI 原生视频路径
        .route(
            "/videos/text2video",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/videos/image2video",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/videos/multi-image2video",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/videos/omni-video",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/videos/text2video/{task_id}",
            get(crate::relay::task::task_status),
        )
        .route(
            "/videos/image2video/{task_id}",
            get(crate::relay::task::task_status),
        )
        .route(
            "/videos/multi-image2video/{task_id}",
            get(crate::relay::task::task_status),
        )
        .route(
            "/videos/omni-video/{task_id}",
            get(crate::relay::task::task_status),
        )
        // 可灵 AI 原生图片路径
        .route(
            "/images/omni-image",
            post(crate::relay::image::image_generations),
        )
        .route(
            "/images/multi-image2image",
            post(crate::relay::image::image_generations),
        )
        .route(
            "/images/omni-image/{task_id}",
            get(crate::relay::task::task_status),
        )
        .route(
            "/images/multi-image2image/{task_id}",
            get(crate::relay::task::task_status),
        )
        .route(
            "/images/generations/{task_id}",
            get(crate::relay::task::task_status),
        )
        // 余额查询
        .route("/balance", get(crate::relay::balance::token_balance))
        .route("/user/balance", get(crate::relay::balance::user_balance))
        // 语音合成
        .route("/audio/speech", post(crate::relay::audio::audio_speech))
        // 模型列表
        .route("/models", get(crate::relay::model_list::list_models))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            api_key_middleware,
        ))
        .with_state(state.clone());

    // 4. 阿里百炼 DashScope Native Relay (绝对路径，直接 merge)
    let dashscope_native_routes: Router<Arc<AppState>> = Router::new()
        .route(
            "/api/v1/services/aigc/video-generation/video-synthesis",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/api/v1/services/aigc/multimodal-generation/generation",
            post(crate::relay::image::image_generations),
        )
        .route(
            "/api/v1/tasks/{task_id}",
            get(crate::relay::task::task_status),
        )
        // 阿里百炼 DashScope 文本向量（OpenAI 兼容模式）
        .route(
            "/compatible-mode/v1/embeddings",
            post(crate::relay::generic::generic_relay),
        )
        // 阿里百炼 DashScope 排序（兼容模式，qwen3-rerank）
        .route(
            "/compatible-api/v1/reranks",
            post(crate::relay::generic::generic_relay),
        )
        // 阿里百炼 DashScope 排序（原生模式，gte-rerank-v2）
        .route(
            "/api/v1/services/rerank/text-rerank/text-rerank",
            post(crate::relay::generic::generic_relay),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            api_key_middleware,
        ))
        .with_state(state.clone());

    // 5. Google Gemini Native Relay (supports ?key=, x-goog-api-key, and Bearer auth)
    let google_native_routes: Router<Arc<AppState>> = Router::new()
        .route(
            "/v1beta/models/{model_action}",
            post(crate::relay::native::gemini_proxy),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            api_key_middleware,
        ))
        .with_state(state.clone());

    // 6. Volcengine Native Relay (火山方舟原生路径)
    let volcengine_native_routes: Router<Arc<AppState>> = Router::new()
        .route(
            "/api/v3/chat/completions",
            post(crate::relay::chat::chat_completions),
        )
        .route(
            "/api/v3/responses",
            post(crate::relay::chat::responses_create),
        )
        .route(
            "/api/v3/contents/generations/tasks",
            post(crate::relay::video::video_generations)
                .get(crate::relay::native::volcengine_task_list),
        )
        .route(
            "/api/v3/contents/generations/tasks/{task_id}",
            get(crate::relay::task::task_status)
                .delete(crate::relay::native::volcengine_task_cancel),
        )
        .route(
            "/api/v3/images/generations",
            post(crate::relay::image::image_generations),
        )
        // 火山方舟原生语音合成路由（SSE + HTTP Chunked 两种传输协议）
        .route(
            "/api/v3/tts/unidirectional/sse",
            post(crate::relay::audio::audio_speech),
        )
        .route(
            "/api/v3/tts/unidirectional",
            post(crate::relay::audio::audio_speech),
        )
        // 模型列表
        .route("/api/v3/models", get(crate::relay::model_list::list_models))
        .route("/api", post(crate::relay::native::ark_asset_proxy))
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            api_key_middleware,
        ))
        .with_state(state.clone());

    let public_router = Router::new()
        .route("/api/health", get(|| async { "OK" }))
        .route("/favicon.ico", get(|| async { "" }));

    // 注册火山引擎画质增强与字幕擦除等专用接口路由，绑定通用视频生成和任务查询处理器，任务查询接口跟阿里百炼一样，并使用 api_key_middleware 鉴权
    let tools_routes = Router::new()
        .route(
            "/tools/enhance-video",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/tools/enhance-video-fast",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/tools/enhance-video-generative",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/tools/erase-video-subtitle-pro",
            post(crate::relay::video::video_generations),
        )
        .route(
            "/tools/erase-video-subtitle",
            post(crate::relay::video::video_generations),
        )
        .route_layer(axum_middleware::from_fn_with_state(
            state.clone(),
            api_key_middleware,
        ))
        .with_state(state.clone());

    let app = Router::new().merge(public_router);
    #[cfg(feature = "plugin_site_portal")]
    let app = app.nest("/home", plugins::site_portal::portal_pages_router());
    #[cfg(feature = "commercial_plugins")]
    let app = app.nest("/home-pro", plugins::site_portal_pro::portal_pages_router());
    let app = app
        .nest("/api/v1/auth", auth_routes)
        .nest("/api/v1", public_v1_routes)
        .nest("/api/v1", management_routes)
        .nest("/api/v1", tools_routes)
        .nest("/v1", relay_routes)
        .merge(dashscope_native_routes)
        .merge(google_native_routes)
        .merge(volcengine_native_routes)
        .with_state(state)
        // CORS：生产环境必须设置 CORS_ORIGINS；开发环境（APP_ENV=development|dev）未设置时允许所有来源
        .layer({
            let origins_env = std::env::var("CORS_ORIGINS").ok();
            let app_env = std::env::var("APP_ENV").unwrap_or_default().to_lowercase();
            let is_dev = matches!(app_env.as_str(), "development" | "dev");
            match origins_env {
                Some(origins) if !origins.trim().is_empty() => {
                    let allowed: Vec<axum::http::HeaderValue> = origins
                        .split(',')
                        .filter_map(|o| o.trim().parse().ok())
                        .collect();
                    tower_http::cors::CorsLayer::new()
                        .allow_origin(allowed)
                        .allow_methods(tower_http::cors::Any)
                        .allow_headers(tower_http::cors::Any)
                }
                _ if is_dev => {
                    tracing::warn!(
                        "CORS_ORIGINS unset with APP_ENV={}; using permissive CORS (dev only)",
                        app_env
                    );
                    tower_http::cors::CorsLayer::permissive()
                }
                _ => {
                    tracing::warn!(
                        "CORS_ORIGINS unset outside development — denying cross-origin browser access. Set CORS_ORIGINS to your frontend origin(s)."
                    );
                    // 不调用 allow_origin：默认拒绝跨域
                    tower_http::cors::CorsLayer::new()
                        .allow_methods(tower_http::cors::Any)
                        .allow_headers(tower_http::cors::Any)
                }
            }
        })
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024));
    app
}
